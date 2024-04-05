use crate::Instruction;
use std::sync::Arc;
use u256::U256;

// An internal representation that doesn't need two Arcs would be better
// but it would also require a lot of unsafe, so I made this wrapper to
// enable changing the internals later.

/// Cloning this is cheap. It is a handle to memory similar to [std::sync::Arc].
#[derive(Clone)]
pub struct Program {
    code_page: Arc<[U256]>,
    instructions: Arc<[Instruction]>,
}

impl Program {
    pub fn new(instructions: Vec<Instruction>, code_page: Vec<U256>) -> Self {
        Self {
            code_page: code_page.into(),
            instructions: instructions.into(),
        }
    }

    pub fn instructions(&self) -> &Arc<[Instruction]> {
        &self.instructions
    }

    pub fn code_page(&self) -> &Arc<[U256]> {
        &self.code_page
    }
}
