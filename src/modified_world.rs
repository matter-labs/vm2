use crate::{
    rollback::{Rollback, RollbackableMap},
    Instruction, World,
};
use std::sync::Arc;
use u256::{H160, U256};

/// The global state including pending modifications that are written only at
/// the end of a block.
pub struct ModifiedWorld<W: World> {
    world: W,
    storage_changes: RollbackableMap<(H160, U256), U256>,
    snapshots: Vec<<RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot>,
}

impl<W: World> ModifiedWorld<W> {
    pub fn new(world: W) -> Self {
        Self {
            world,
            storage_changes: Default::default(),
            snapshots: vec![],
        }
    }

    pub fn snapshot(&mut self) {
        self.snapshots.push(self.storage_changes.snapshot())
    }

    pub fn rollback(&mut self) {
        self.storage_changes.rollback(self.snapshots.pop().unwrap())
    }

    pub fn forget_snapshot(&mut self) {
        self.storage_changes.forget(self.snapshots.pop().unwrap())
    }

    pub fn read_storage(&mut self, contract: H160, key: U256) -> U256 {
        self.storage_changes
            .as_ref()
            .get(&(contract, key))
            .cloned()
            .unwrap_or_else(|| self.world.read_storage(contract, key))
    }

    pub fn write_storage(&mut self, contract: H160, key: U256, value: U256) {
        self.storage_changes
            .insert((contract, key), value, self.snapshots.is_empty())
    }

    pub fn decommit(&mut self) -> (Arc<[Instruction<W>]>, Arc<[U256]>) {
        self.world.decommit()
    }
}
