use std::collections::BTreeMap;

use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap, RollbackableSet},
    World,
};
use u256::{H160, U256};
use zkevm_opcode_defs::system_params::{
    STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST, STORAGE_ACCESS_WARM_READ_COST,
    STORAGE_ACCESS_WARM_WRITE_COST,
};

/// The global state including pending modifications that are written only at
/// the end of a block.
pub struct ModifiedWorld {
    pub(crate) world: Box<dyn World>,

    // These are rolled back on revert or panic (and when the whole VM is rolled back).
    storage_changes: RollbackableMap<(H160, U256), U256>,
    events: RollbackableLog<Event>,

    // The field below are only rolled back when the whole VM is rolled back.
    pub(crate) decommitted_hashes: RollbackableSet<U256>,
    read_storage_slots: RollbackableSet<(H160, U256)>,
    written_storage_slots: RollbackableSet<(H160, U256)>,
}

pub struct ExternalSnapshot {
    storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    events: <RollbackableLog<Event> as Rollback>::Snapshot,

    // The field below are only rolled back when the whole VM is rolled back.
    pub(crate) decommitted_hashes: <RollbackableMap<U256, ()> as Rollback>::Snapshot,
    read_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    written_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u16,
}

impl ModifiedWorld {
    pub fn new(world: Box<dyn World>) -> Self {
        Self {
            world,
            storage_changes: Default::default(),
            events: Default::default(),
            decommitted_hashes: Default::default(),
            read_storage_slots: Default::default(),
            written_storage_slots: Default::default(),
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

        let refund = if self.read_storage_slots.contains(&(contract, key)) {
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

        if self
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

    pub fn get_storage_changes(&self) -> &BTreeMap<(H160, U256), U256> {
        self.storage_changes.as_ref()
    }

    pub fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        (self.storage_changes.snapshot(), self.events.snapshot())
    }

    pub(crate) fn rollback(&mut self, (storage, events): Snapshot) {
        self.storage_changes.rollback(storage);
        self.events.rollback(events);
    }

    pub fn external_snapshot(&mut self) -> ExternalSnapshot {
        ExternalSnapshot {
            storage_changes: self.storage_changes.snapshot(),
            events: self.events.snapshot(),
            decommitted_hashes: self.decommitted_hashes.snapshot(),
            read_storage_slots: self.read_storage_slots.snapshot(),
            written_storage_slots: self.written_storage_slots.snapshot(),
        }
    }

    pub fn external_rollback(&mut self, snapshot: ExternalSnapshot) {
        self.storage_changes.rollback(snapshot.storage_changes);
        self.events.rollback(snapshot.events);
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
}

pub(crate) type Snapshot = (
    <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    <RollbackableLog<Event> as Rollback>::Snapshot,
);

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;
