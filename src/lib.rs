pub mod addressing_modes;
#[cfg(feature = "arbitrary")]
mod arbitrary_instruction;
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
mod predication;
#[cfg(not(feature = "single_instruction_test"))]
mod program;
mod rollback;
#[cfg(not(feature = "single_instruction_test"))]
mod stack;
mod state;
pub mod testworld;
mod vm;
mod world_diff;

use u256::{H160, U256};

pub use decommit::address_into_u256;
pub use decommit::initial_decommit;
pub use heap::{HeapId, FIRST_HEAP};
pub use instruction::{jump_to_beginning, ExecutionEnd, Instruction};
pub use predication::Predicate;
pub use program::Program;
pub use state::State;
pub use vm::{Settings, VirtualMachine, VmSnapshot as Snapshot};
pub use world_diff::{Event, L2ToL1Log, WorldDiff};

#[cfg(feature = "single_instruction_test")]
mod single_instruction_test;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::heap;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::program;
#[cfg(feature = "single_instruction_test")]
use single_instruction_test::stack;
#[cfg(feature = "single_instruction_test")]
pub use single_instruction_test::MockWorld;
#[cfg(feature = "single_instruction_test")]
pub use zkevm_opcode_defs;

pub trait World {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program;

    /// There is no write_storage; [WorldDiff::get_storage_changes] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> Option<U256>;

    /// Computes the cost of writing a storage slot.
    fn cost_of_writing_storage(&mut self, initial_value: Option<U256>, new_value: U256) -> u32;

    /// Returns if the storage slot is free both in terms of gas and pubdata.
    fn is_free_storage_slot(&self, contract: &H160, key: &U256) -> bool;
}
