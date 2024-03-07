pub mod addressing_modes;
mod bitset;
pub mod decode;
mod decommit;
mod fat_pointer;
pub mod instruction_handlers;
mod modified_world;
mod predication;
mod rollback;
mod state;

use std::sync::Arc;
use u256::{H160, U256};

pub use decommit::address_into_u256;
pub use modified_world::Event;
pub use predication::Predicate;
pub use state::{
    end_execution, jump_to_beginning, run_arbitrary_program, ExecutionEnd, Instruction, State,
};

pub trait World {
    /// This will be called *every* time a contract is called. Caching and decoding is
    /// the world implementor's job.
    fn decommit(&mut self, hash: U256) -> (Arc<[Instruction]>, Arc<[U256]>);

    /// There is no write_storage; [ModifiedWorld::get_storage_changes] gives a list of all storage changes.
    fn read_storage(&mut self, contract: H160, key: U256) -> U256;
}
