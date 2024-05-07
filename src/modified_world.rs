use std::collections::BTreeMap;

use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap, RollbackableSet},
    World,
};
use u256::{H160, H256, U256};
use zkevm_opcode_defs::{
    blake2::Blake2s256,
    sha3::Digest,
    system_params::{
        STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST,
        STORAGE_ACCESS_WARM_READ_COST, STORAGE_ACCESS_WARM_WRITE_COST,
    },
};

/// The global state including pending modifications that are written only at
/// the end of a block.
pub struct ModifiedWorld {
    pub(crate) world: Box<dyn World>,

    // These are rolled back on revert or panic (and when the whole VM is rolled back).
    storage_changes: RollbackableMap<(H160, U256), U256>,
    events: RollbackableLog<Event>,
    l2_to_l1_logs: RollbackableLog<L2ToL1Log>,
    paid_changes: RollbackableMap<(H160, U256), u32>,
    initial_values: RollbackableMap<(H160, U256), U256>,

    // The fields below are only rolled back when the whole VM is rolled back.
    pub(crate) decommitted_hashes: RollbackableSet<U256>,
    read_storage_slots: RollbackableSet<(H160, U256)>,
    written_storage_slots: RollbackableSet<(H160, U256)>,
}

pub struct ExternalSnapshot {
    internal_snapshot: Snapshot,
    pub(crate) decommitted_hashes: <RollbackableMap<U256, ()> as Rollback>::Snapshot,
    read_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    written_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
}

/// There is no address field because nobody is interested in events that don't come
/// from the event writer, so we simply do not record events coming frome anywhere else.
#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u16,
}

pub struct L2ToL1Log {
    pub key: U256,
    pub value: U256,
    pub is_service: bool,
    pub address: H160,
    pub shard_id: u8,
    pub tx_number: u16,
}

impl ModifiedWorld {
    pub fn new(world: Box<dyn World>) -> Self {
        Self {
            world,
            storage_changes: Default::default(),
            events: Default::default(),
            l2_to_l1_logs: Default::default(),
            decommitted_hashes: Default::default(),
            read_storage_slots: Default::default(),
            written_storage_slots: Default::default(),
            paid_changes: Default::default(),
            initial_values: Default::default(),
        }
    }

    /// Returns the storage slot's value and a refund based on its hot/cold status.
    pub fn read_storage(&mut self, contract: H160, key: U256) -> (U256, u32) {
        let value = self
            .storage_changes
            .as_ref()
            .get(&(contract, key))
            .cloned()
            .unwrap_or_else(|| self.world.read_storage(contract, key));

        let refund = if is_storage_key_free(&contract, &key)
            || self.read_storage_slots.contains(&(contract, key))
        {
            WARM_READ_REFUND
        } else {
            self.read_storage_slots.add((contract, key));
            0
        };

        (value, refund)
    }

    /// Returns the refund based the hot/cold status of the storage slot.
    pub fn write_storage(&mut self, contract: H160, key: U256, value: U256) -> u32 {
        self.storage_changes.insert((contract, key), value);

        if is_storage_key_free(&contract, &key)
            || self
                .written_storage_slots
                .as_ref()
                .contains_key(&(contract, key))
        {
            WARM_WRITE_REFUND
        } else {
            self.written_storage_slots.add((contract, key));

            if self.read_storage_slots.contains(&(contract, key)) {
                COLD_WRITE_AFTER_WARM_READ_REFUND
            } else {
                self.read_storage_slots.add((contract, key));
                0
            }
        }
    }

    pub fn prepaid_for_write(&self, address: H160, key: U256) -> u32 {
        self.paid_changes
            .as_ref()
            .get(&(address, key))
            .cloned()
            .unwrap_or(0u32)
    }

    pub fn insert_prepaid_for_write(&mut self, address: H160, key: U256, price: u32) {
        self.paid_changes.insert((address, key), price)
    }

    pub fn set_initial_value(&mut self, address: H160, key: U256, value: U256) {
        if !self.initial_values.as_ref().contains_key(&(address, key)) {
            self.initial_values.insert((address, key), value);
        }
    }

    pub fn get_initial_value(&self, address: &H160, key: &U256) -> Option<U256> {
        self.initial_values.as_ref().get(&(*address, *key)).copied()
    }

    pub fn get_storage_changes(&self) -> &BTreeMap<(H160, U256), U256> {
        self.storage_changes.as_ref()
    }

    pub(crate) fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    pub(crate) fn record_l2_to_l1_log(&mut self, log: L2ToL1Log) {
        self.l2_to_l1_logs.push(log);
    }

    pub fn l2_to_l1_logs(&self) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.as_ref()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        (
            self.storage_changes.snapshot(),
            self.events.snapshot(),
            self.l2_to_l1_logs.snapshot(),
            self.paid_changes.snapshot(),
        )
    }

    pub(crate) fn rollback(&mut self, (storage, events, l2_to_l1_logs, paid_changes): Snapshot) {
        self.storage_changes.rollback(storage);
        self.events.rollback(events);
        self.l2_to_l1_logs.rollback(l2_to_l1_logs);
        self.paid_changes.rollback(paid_changes);
    }

    /// This function must only be called during the initial frame
    /// because otherwise internal rollbacks can roll back past the external snapshot.
    pub(crate) fn external_snapshot(&self) -> ExternalSnapshot {
        ExternalSnapshot {
            internal_snapshot: self.snapshot(),
            decommitted_hashes: self.decommitted_hashes.snapshot(),
            read_storage_slots: self.read_storage_slots.snapshot(),
            written_storage_slots: self.written_storage_slots.snapshot(),
        }
    }

    pub(crate) fn external_rollback(&mut self, snapshot: ExternalSnapshot) {
        self.rollback(snapshot.internal_snapshot);
        self.decommitted_hashes
            .rollback(snapshot.decommitted_hashes);
        self.read_storage_slots
            .rollback(snapshot.read_storage_slots);
        self.written_storage_slots
            .rollback(snapshot.written_storage_slots);
    }

    /// This must only be called when it is known that the VM cannot be rolled back,
    /// so there must not be any external snapshots and the callstack
    /// should ideally be empty, though in practice it sometimes contains
    /// a near call inside the bootloader.
    pub fn delete_history(&mut self) {
        self.storage_changes.delete_history();
        self.events.delete_history();
        self.decommitted_hashes.delete_history();
        self.read_storage_slots.delete_history();
        self.written_storage_slots.delete_history();
    }

    pub(crate) fn is_write_initial(&self, address: H160, key: U256) -> bool {
        self.read_storage_slots.contains(&(address, key))
    }
}

pub(crate) type Snapshot = (
    <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    <RollbackableLog<Event> as Rollback>::Snapshot,
    <RollbackableLog<L2ToL1Log> as Rollback>::Snapshot,
    <RollbackableMap<(H160, U256), u32> as Rollback>::Snapshot,
);

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;

pub(crate) fn is_storage_key_free(address: &H160, key: &U256) -> bool {
    let storage_key_for_eth_balance = U256([
        4209092924407300373,
        6927221427678996148,
        4194905989268492595,
        15931007429432312239,
    ]);
    if address == &SYSTEM_CONTEXT_ADDRESS {
        return true;
    }

    let keyy = U256::from_little_endian(&raw_hashed_key(&address, &u256_to_h256(*key)));

    if keyy == storage_key_for_eth_balance {
        return true;
    }

    false
}

pub const SYSTEM_CONTEXT_ADDRESS: H160 = H160([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x80, 0x0b,
]);

fn u256_to_h256(num: U256) -> H256 {
    let mut bytes = [0u8; 32];
    num.to_big_endian(&mut bytes);
    H256::from_slice(&bytes)
}

fn raw_hashed_key(address: &H160, key: &H256) -> [u8; 32] {
    let mut bytes = [0u8; 64];
    bytes[12..32].copy_from_slice(&address.0);
    U256::from(key.to_fixed_bytes()).to_big_endian(&mut bytes[32..64]);

    Blake2s256::digest(bytes).into()
}
