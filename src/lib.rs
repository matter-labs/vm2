pub mod addressing_modes;
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
mod state;
mod vm;

use u256::{H160, U256};

pub use decommit::address_into_u256;
pub use instruction::{jump_to_beginning, run_arbitrary_program, ExecutionEnd, Instruction};
pub use modified_world::Event;
pub use predication::Predicate;
pub use program::Program;
pub use state::State;
pub use vm::{Settings, VirtualMachine};

pub trait World {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> Program;

    /// There is no write_storage; [ModifiedWorld::get_storage_changes] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> U256;

    /// Called with values that the bootloader writes to the heap location [Settings::hook_address].
    fn handle_hook(&mut self, hook: u32, state: &mut State);
}
