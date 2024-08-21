pub mod addressing_modes;
#[cfg(not(feature = "single_instruction_test"))]
mod bitset;
mod callframe;
pub mod decode;
mod decommit;
pub mod fat_pointer;
#[cfg(not(feature = "single_instruction_test"))]
mod heap;
mod instruction;
pub mod instruction_handlers;
mod mode_requirements;
mod predication;
#[cfg(not(feature = "single_instruction_test"))]
mod program;
mod rollback;
#[cfg(not(feature = "single_instruction_test"))]
mod stack;
mod state;
pub mod testworld;
mod tracing;
mod vm;
mod world_diff;

use std::hash::{DefaultHasher, Hash, Hasher};
use u256::{H160, U256};

pub use decommit::address_into_u256;
pub use decommit::initial_decommit;
pub use eravm_stable_interface::{
    CallframeInterface, HeapId, Opcode, OpcodeType, StateInterface, Tracer,
};
pub use heap::FIRST_HEAP;
pub use instruction::{jump_to_beginning, ExecutionEnd, Instruction};
pub use mode_requirements::ModeRequirements;
pub use predication::Predicate;
pub use program::Program;
pub use state::State;
pub use vm::{Settings, VirtualMachine, VmSnapshot as Snapshot};
pub use world_diff::{Event, L2ToL1Log, WorldDiff};

#[cfg(feature = "single_instruction_test")]
pub mod single_instruction_test;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::heap;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::program;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::stack;
#[cfg(feature = "single_instruction_test")]
pub use zkevm_opcode_defs;

pub trait World<T>: StorageInterface + Sized {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program<T, Self>;

    fn decommit_code(&mut self, hash: U256) -> Vec<u8>;
}

pub trait StorageInterface {
    /// There is no write_storage; [WorldDiff::get_storage_changes] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> Option<U256>;

    /// Computes the cost of writing a storage slot.
    fn cost_of_writing_storage(&mut self, initial_value: Option<U256>, new_value: U256) -> u32;

    /// Returns if the storage slot is free both in terms of gas and pubdata.
    fn is_free_storage_slot(&self, contract: &H160, key: &U256) -> bool;
}

/// Deterministic (across program runs and machines) hash that can be used for `Debug` implementations
/// to concisely represent large amounts of data.
#[cfg_attr(feature = "single_instruction_test", allow(dead_code))] // Currently used entirely in types overridden by `single_instruction_test` feature
pub(crate) fn hash_for_debugging(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
