use crate::{
    addressing_modes::{Arguments, Register1, Register2, Source},
    state::ExecutionResult,
    Instruction, Predicate, State,
};

use super::common::run_next_instruction;

fn event(state: &mut State, instruction: *const Instruction) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    let key = Register1::get(args, state);
    let value = Register2::get(args, state);

    dbg!(key, value);

    run_next_instruction(state, instruction)
}

impl Instruction {
    pub fn from_event(key: Register1, value: Register2, predicate: Predicate) -> Self {
        Self {
            handler: event,
            arguments: Arguments::new(predicate)
                .write_source(&key)
                .write_source(&value),
        }
    }
}
