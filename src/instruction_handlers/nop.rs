use super::common::instruction_boilerplate;
use crate::{addressing_modes::Arguments, Instruction, State};

fn nop(state: &mut State, instruction: *const Instruction) {
    instruction_boilerplate(state, instruction, |_, _| {});
}

impl Instruction {
    pub fn from_nop() -> Self {
        Self {
            handler: nop,
            arguments: Arguments::default(),
        }
    }
}
