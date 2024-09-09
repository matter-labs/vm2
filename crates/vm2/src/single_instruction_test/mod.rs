//! Code required to efficiently explore all possible behaviours of a single instruction.
//!
//! It would be wasteful to randomly generate the whole heap. Instead, we only generate
//! the part of the heap that is actually accessed, which is at most 32 bytes!
//!
//! The same kind of mocking in applied to stack memory, the program, the world and callstack.

pub use self::{
    into_zk_evm::{add_heap_to_zk_evm, vm2_to_zk_evm, NoTracer},
    universal_state::UniversalVmState,
    world::MockWorld,
};

mod callframe;
pub mod heap;
mod into_zk_evm;
mod mock_array;
mod print_mock_info;
pub mod program;
pub mod stack;
mod state_to_zk_evm;
mod universal_state;
mod validation;
mod vm;
mod world;
