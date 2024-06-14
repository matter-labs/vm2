use crate::Instruction;
use std::sync::Arc;
use u256::U256;

// An internal representation that doesn't need two Arcs would be better
// but it would also require a lot of unsafe, so I made this wrapper to
// enable changing the internals later.

/// Cloning this is cheap. It is a handle to memory similar to [std::sync::Arc].
#[derive(Clone, Debug)]
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

    pub fn instruction(&self, n: u16) -> Option<&Instruction> {
        self.instructions.get::<usize>(n.into())
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn code_page(&self) -> &Arc<[U256]> {
        &self.code_page
    }
}

// This implementation compares pointers instead of programs.
//
// That works well enough for the tests that this is written for.
// I don't want to implement PartialEq for Instruction because
// comparing function pointers can work in suprising ways.
impl PartialEq for Program {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.code_page, &other.code_page)
            && Arc::ptr_eq(&self.instructions, &other.instructions)
    }
}
