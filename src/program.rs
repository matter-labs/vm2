use crate::{hash_for_debugging, Instruction};
use std::{fmt, sync::Arc};
use u256::U256;

// An internal representation that doesn't need two Arcs would be better
// but it would also require a lot of unsafe, so I made this wrapper to
// enable changing the internals later.

/// Cloning this is cheap. It is a handle to memory similar to [`Arc`].
pub struct Program<T, W> {
    code_page: Arc<[U256]>,
    instructions: Arc<[Instruction<T, W>]>,
}

impl<T, W> Clone for Program<T, W> {
    fn clone(&self) -> Self {
        Self {
            code_page: self.code_page.clone(),
            instructions: self.instructions.clone(),
        }
    }
}

impl<T, W> fmt::Debug for Program<T, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEBUGGED_ITEMS: usize = 16;

        let mut s = formatter.debug_struct("Program");
        if self.code_page.len() <= DEBUGGED_ITEMS {
            s.field("code_page", &self.code_page);
        } else {
            s.field("code_page.len", &self.code_page.len())
                .field("code_page.start", &&self.code_page[..DEBUGGED_ITEMS])
                .field("code_page.hash", &hash_for_debugging(&self.code_page));
        }

        if self.instructions.len() <= DEBUGGED_ITEMS {
            s.field("instructions", &self.instructions);
        } else {
            s.field("instructions.len", &self.instructions.len())
                .field("instructions.start", &&self.instructions[..DEBUGGED_ITEMS]);
        }
        s.finish_non_exhaustive()
    }
}

impl<T, W> Program<T, W> {
    pub fn new(instructions: Vec<Instruction<T, W>>, code_page: Vec<U256>) -> Self {
        Self {
            code_page: code_page.into(),
            instructions: instructions.into(),
        }
    }

    pub fn instruction(&self, n: u16) -> Option<&Instruction<T, W>> {
        self.instructions.get::<usize>(n.into())
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
impl<T, W> PartialEq for Program<T, W> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.code_page, &other.code_page)
            && Arc::ptr_eq(&self.instructions, &other.instructions)
    }
}
