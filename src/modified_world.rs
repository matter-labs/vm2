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
    l2_to_l1_logs: RollbackableLog<L2ToL1Log>,
    paid_changes: RollbackableMap<(H160, U256), u32>,

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
        }
    }

    /// Returns the storage slot's value and a refund based on its hot/cold status.
    pub(crate) fn read_storage(&mut self, contract: H160, key: U256) -> (U256, u32) {
        let value = self
            .storage_changes
            .as_ref()
            .get(&(contract, key))
            .cloned()
            .unwrap_or_else(|| self.world.read_storage(contract, key));

        let refund = if self.world.is_free_storage_slot(&contract, &key)
            || self.read_storage_slots.contains(&(contract, key))
        {
            WARM_READ_REFUND
        } else {
            self.read_storage_slots.add((contract, key));
            0
        };

        (value, refund)
    }

    /// Returns the refund based the hot/cold status of the storage slot and the change in pubdata.
    pub(crate) fn write_storage(&mut self, contract: H160, key: U256, value: U256) -> (u32, i32) {
        self.storage_changes.insert((contract, key), value);

        if self.world.is_free_storage_slot(&contract, &key) {
            return (WARM_WRITE_REFUND, 0);
        }

        let update_cost = self.world.cost_of_writing_storage(contract, key, value);
        let prepaid = self
            .paid_changes
            .insert((contract, key), update_cost)
            .unwrap_or(0);

        let refund = if self
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
        };

        (refund, (update_cost as i32) - (prepaid as i32))
    }

    pub fn get_storage_state(&self) -> &BTreeMap<(H160, U256), U256> {
        self.storage_changes.as_ref()
    }

    pub fn get_storage_changes(&self) -> BTreeMap<(H160, U256), (Option<U256>, U256)> {
        self.storage_changes.changes_after(0)
    }

    pub fn get_storage_changes_after(
        &self,
        snapshot: &Snapshot,
    ) -> BTreeMap<(H160, U256), (Option<U256>, U256)> {
        self.storage_changes.changes_after(snapshot.storage_changes)
    }

    pub(crate) fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    pub fn events_after(&self, snapshot: &Snapshot) -> &[Event] {
        self.events.logs_after(snapshot.events)
    }

    pub(crate) fn record_l2_to_l1_log(&mut self, log: L2ToL1Log) {
        self.l2_to_l1_logs.push(log);
    }

    pub fn l2_to_l1_logs(&self) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.as_ref()
    }

    pub fn l2_to_l1_logs_after(&self, snapshot: &Snapshot) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.logs_after(snapshot.l2_to_l1_logs)
    }

    /// Get a snapshot for selecting which logs [Self::events_after] & Co output.
    /// The snapshot can't be used for rolling back the VM because the method for
    /// that is private. Use [crate::VirtualMachine::snapshot] for that instead.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            storage_changes: self.storage_changes.snapshot(),
            events: self.events.snapshot(),
            l2_to_l1_logs: self.l2_to_l1_logs.snapshot(),
            paid_changes: self.paid_changes.snapshot(),
        }
    }

    pub(crate) fn rollback(
        &mut self,
        Snapshot {
            storage_changes,
            events,
            l2_to_l1_logs,
            paid_changes,
        }: Snapshot,
    ) {
        self.storage_changes.rollback(storage_changes);
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
}

#[derive(Clone, PartialEq, Debug)]
pub struct Snapshot {
    storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    events: <RollbackableLog<Event> as Rollback>::Snapshot,
    l2_to_l1_logs: <RollbackableLog<L2ToL1Log> as Rollback>::Snapshot,
    paid_changes: <RollbackableMap<(H160, U256), u32> as Rollback>::Snapshot,
}

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;
