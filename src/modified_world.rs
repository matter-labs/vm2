use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap},
    Instruction, World,
};
use std::sync::Arc;
use u256::{H160, U256};

/// The global state including pending modifications that are written only at
/// the end of a block.
pub struct ModifiedWorld {
    world: Box<dyn World>,
    storage_changes: RollbackableMap<(H160, U256), U256>,
    decommitted_hashes: RollbackableMap<U256, ()>,
    events: RollbackableLog<Event>,
    snapshots: Vec<(
        <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
        <RollbackableMap<U256, ()> as Rollback>::Snapshot,
        <RollbackableLog<Event> as Rollback>::Snapshot,
    )>,
}

pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u32,
}

impl World for ModifiedWorld {
    fn decommit(&mut self, hash: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
        self.decommitted_hashes
            .insert(hash, (), self.snapshots.is_empty());
        self.world.decommit(hash)
    }

    fn read_storage(&mut self, contract: H160, key: U256) -> U256 {
        self.storage_changes
            .as_ref()
            .get(&(contract, key))
            .cloned()
            .unwrap_or_else(|| self.world.read_storage(contract, key))
    }
}

impl ModifiedWorld {
    pub fn new(world: Box<dyn World>) -> Self {
        Self {
            world,
            storage_changes: Default::default(),
            decommitted_hashes: Default::default(),
            events: Default::default(),
            snapshots: vec![],
        }
    }

    pub fn snapshot(&mut self) {
        self.snapshots.push((
            self.storage_changes.snapshot(),
            self.decommitted_hashes.snapshot(),
            self.events.snapshot(),
        ))
    }

    pub fn rollback(&mut self) {
        let (storage, decommit, events) = self.snapshots.pop().unwrap();
        self.storage_changes.rollback(storage);
        self.decommitted_hashes.rollback(decommit);
        self.events.rollback(events);
    }

    pub fn forget_snapshot(&mut self) {
        let (storage, decommit, events) = self.snapshots.pop().unwrap();
        self.storage_changes.forget(storage);
        self.decommitted_hashes.forget(decommit);
        self.events.forget(events);
    }

    pub fn write_storage(&mut self, contract: H160, key: U256, value: U256) {
        self.storage_changes
            .insert((contract, key), value, self.snapshots.is_empty())
    }

    pub fn get_storage_changes(&self) -> impl Iterator<Item = ((H160, U256), U256)> + '_ {
        self.storage_changes.as_ref().iter().map(|(k, v)| (*k, *v))
    }

    pub fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        self.events.as_ref()
    }
}
