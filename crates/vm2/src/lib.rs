use std::hash::{DefaultHasher, Hash, Hasher};

use primitive_types::{H160, U256};
pub use zksync_vm2_interface::{
    CallframeInterface, CycleStats, HeapId, Opcode, OpcodeType, ReturnType, StateInterface, Tracer,
};

// Re-export missing modules if single instruction testing is enabled
#[cfg(feature = "single_instruction_test")]
pub(crate) use self::single_instruction_test::{heap, program, stack};
pub use self::{
    decommit::{address_into_u256, initial_decommit},
    fat_pointer::FatPointer,
    instruction::{ExecutionEnd, Instruction},
    mode_requirements::ModeRequirements,
    predication::Predicate,
    program::Program,
    vm::{Settings, VirtualMachine},
    world_diff::{Event, L2ToL1Log, WorldDiff},
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
pub mod testworld;
mod tracing;
mod vm;
mod world_diff;

/// VM storage access operations.
pub trait StorageInterface {
    /// Reads the specified slot from the storage.
    ///
    /// There is no write counterpart; [`WorldDiff::get_storage_changes()`] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> Option<U256>;

    /// Computes the cost of writing a storage slot.
    fn cost_of_writing_storage(&mut self, initial_value: Option<U256>, new_value: U256) -> u32;

    /// Returns if the storage slot is free both in terms of gas and pubdata.
    fn is_free_storage_slot(&self, contract: &H160, key: &U256) -> bool;
}

pub trait World<T: Tracer>: StorageInterface + Sized {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program<T, Self>;

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
