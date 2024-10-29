//! # High-Performance ZKsync Era VM
//!
//! This crate provides high-performance [`VirtualMachine`] for ZKsync Era.

use std::hash::{DefaultHasher, Hash, Hasher};

use primitive_types::{H160, U256};
pub use zksync_vm2_interface as interface;
pub use zksync_vm2_interface::ExecutionEnd;
use zksync_vm2_interface::Tracer;

// Re-export missing modules if single instruction testing is enabled
#[cfg(feature = "single_instruction_test")]
pub(crate) use self::single_instruction_test::{heap, program, stack};
pub use self::{
    fat_pointer::FatPointer,
    instruction::Instruction,
    mode_requirements::ModeRequirements,
    predication::Predicate,
    program::Program,
    vm::{Settings, VirtualMachine},
    world_diff::{Snapshot, StorageChange, WorldDiff},
};

pub mod addressing_modes;
#[cfg(not(feature = "single_instruction_test"))]
mod bitset;
mod callframe;
mod decode;
mod decommit;
mod fat_pointer;
#[cfg(not(feature = "single_instruction_test"))]
mod heap;
mod instruction;
mod instruction_handlers;
mod mode_requirements;
mod predication;
#[cfg(not(feature = "single_instruction_test"))]
mod program;
mod rollback;
#[cfg(feature = "single_instruction_test")]
pub mod single_instruction_test;
#[cfg(not(feature = "single_instruction_test"))]
mod stack;
mod state;
pub mod testonly;
#[cfg(all(test, not(feature = "single_instruction_test")))]
mod tests;
mod tracing;
mod vm;
mod world_diff;

/// Storage slot information returned from [`StorageInterface::read_storage()`].
#[derive(Debug, Clone, Copy)]
pub struct StorageSlot {
    /// Value of the storage slot.
    pub value: U256,
    /// Whether a write to the slot would be considered an initial write. This influences refunds.
    pub is_write_initial: bool,
}

impl StorageSlot {
    /// Represents an empty storage slot.
    pub const EMPTY: Self = Self {
        value: U256([0; 4]),
        is_write_initial: true,
    };
}

/// VM storage access operations.
pub trait StorageInterface {
    /// Reads the specified slot from the storage.
    ///
    /// There is no write counterpart; [`WorldDiff::get_storage_changes()`] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> StorageSlot;

    /// Same as [`Self::read_storage()`], but doesn't request the initialness flag for the read slot.
    ///
    /// The default implementation uses `read_storage()`.
    fn read_storage_value(&mut self, contract: H160, key: U256) -> U256 {
        self.read_storage(contract, key).value
    }

    /// Computes the cost of writing a storage slot.
    fn cost_of_writing_storage(&mut self, initial_slot: StorageSlot, new_value: U256) -> u32;

    /// Returns if the storage slot is free both in terms of gas and pubdata.
    fn is_free_storage_slot(&self, contract: &H160, key: &U256) -> bool;
}

/// Encapsulates VM interaction with the external world. This includes VM storage and decomitting (loading) bytecodes
/// for execution.
pub trait World<T: Tracer>: StorageInterface + Sized {
    /// Loads a bytecode with the specified hash.
    ///
    /// This method will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program<T, Self>;

    /// Loads bytecode bytes for the `decommit` opcode.
    fn decommit_code(&mut self, hash: U256) -> Vec<u8>;
}

/// Deterministic (across program runs and machines) hash that can be used for `Debug` implementations
/// to concisely represent large amounts of data.
#[cfg_attr(feature = "single_instruction_test", allow(dead_code))] // Currently used entirely in types overridden by `single_instruction_test` feature
pub(crate) fn hash_for_debugging(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
