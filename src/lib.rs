pub mod addressing_modes;
mod bitset;
pub mod decode;
pub mod instruction_handlers;
mod predication;
mod state;

use std::sync::Arc;
use u256::U256;

pub use predication::Predicate;
pub use state::{end_execution, jump_to_beginning, run_arbitrary_program, Instruction, State};

pub trait World: Sized {
    fn decommit(&mut self) -> (Arc<[Instruction<Self>]>, Arc<[U256]>);
    fn read_storage() -> U256;
}
