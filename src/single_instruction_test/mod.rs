//! Code required to efficiently explore all possible behaviours of a single instruction.
//!
//! It would be wasteful to randomly generate the whole heap. Instead, we only generate
//! the part of the heap that is actually accessed, which is at most 32 bytes!
//!
//! The same kind of mocking in applied to stack memory, the program, the world and callstack.

mod callframe;
pub mod heap;
mod mock_array;
pub mod program;
pub mod stack;
mod validation;
mod vm;
mod world;

pub use world::MockWorld;
