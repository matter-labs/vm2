use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap},
    state::State,
    World,
};
use u256::{H160, U256};

/// The global state including pending modifications that are written only at
/// the end of a block.
pub struct ModifiedWorld {
    pub(crate) world: Box<dyn World>,
    storage_changes: RollbackableMap<(H160, U256), U256>,
    pub(crate) decommitted_hashes: RollbackableMap<U256, ()>,
    events: RollbackableLog<Event>,
}

pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u32,
}

impl Rollback for ModifiedWorld {
    type Snapshot = (
        <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
        <RollbackableMap<U256, ()> as Rollback>::Snapshot,
        <RollbackableLog<Event> as Rollback>::Snapshot,
    );

    fn snapshot(&self) -> Self::Snapshot {
        (
            self.storage_changes.snapshot(),
            self.decommitted_hashes.snapshot(),
            self.events.snapshot(),
        )
    }

    fn rollback(&mut self, (storage, decommit, events): Self::Snapshot) {
        self.storage_changes.rollback(storage);
        self.decommitted_hashes.rollback(decommit);
        self.events.rollback(events);
    }

    fn delete_history(&mut self) {
        self.storage_changes.delete_history();
        self.decommitted_hashes.delete_history();
        self.events.delete_history();
    }
}

impl ModifiedWorld {
    pub fn new(world: Box<dyn World>) -> Self {
        Self {
            world,
            storage_changes: Default::default(),
            decommitted_hashes: Default::default(),
            events: Default::default(),
        }
    }

    pub fn read_storage(&mut self, contract: H160, key: U256) -> U256 {
        self.storage_changes
            .as_ref()
            .get(&(contract, key))
            .cloned()
            .unwrap_or_else(|| self.world.read_storage(contract, key))
    }

    pub fn handle_hook(&mut self, value: u32, state: &mut State) {
        self.world.handle_hook(value, state)
    }

    pub fn write_storage(&mut self, contract: H160, key: U256, value: U256) {
        self.storage_changes.insert((contract, key), value)
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
