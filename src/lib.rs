pub mod addressing_modes;
mod bitset;
pub mod decode;
pub mod instruction_handlers;
mod modified_world;
mod predication;
mod rollback;
mod state;

use std::sync::Arc;
use u256::{H160, U256};

pub use predication::Predicate;
pub use state::{end_execution, jump_to_beginning, run_arbitrary_program, Instruction, State};

pub trait World: Sized {
    fn decommit(&mut self) -> (Arc<[Instruction<Self>]>, Arc<[U256]>);

    /// There is no write_storage; the caller must ask for all storage changes when done.
    fn read_storage(&mut self, contract: H160, key: U256) -> U256;
}
