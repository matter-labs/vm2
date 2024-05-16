pub mod addressing_modes;
#[cfg(feature = "arbitrary")]
mod arbitrary_instruction;
mod bitset;
mod callframe;
pub mod decode;
mod decommit;
mod fat_pointer;
mod instruction;
pub mod instruction_handlers;
mod modified_world;
mod predication;
mod program;
mod rollback;
mod stack;
mod state;
pub mod testworld;
mod vm;

use u256::{H160, U256};

pub use decommit::address_into_u256;
pub use decommit::initial_decommit;
pub use instruction::{jump_to_beginning, ExecutionEnd, Instruction};
pub use modified_world::{Event, L2ToL1Log, WorldDiff};
pub use predication::Predicate;
pub use program::Program;
pub use state::{State, FIRST_HEAP};
pub use vm::{Settings, VirtualMachine, VmSnapshot as Snapshot};

pub trait World {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program;

    /// There is no write_storage; [WorldDiff::get_storage_changes] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> U256;

    /// Computes the cost of writing a storage slot.
    fn cost_of_writing_storage(&mut self, contract: H160, key: U256, new_value: U256) -> u32;

    /// Returns if the storage slot is free both in terms of gas and pubdata.
    fn is_free_storage_slot(&self, contract: &H160, key: &U256) -> bool;
}
