use crate::{
    addressing_modes::{Arguments, Register1, Register2, Source},
    instruction_handlers::common::instruction_boilerplate,
    state::InstructionResult,
    Instruction, Predicate, State,
};

fn event(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        let key = Register1::get(args, state);
        let value = Register2::get(args, state);

        dbg!(key, value);
    })
}

impl Instruction {
    pub fn from_event(key: Register1, value: Register2, predicate: Predicate) -> Self {
        Self {
            handler: event,
            arguments: Arguments::new(predicate, 38)
                .write_source(&key)
                .write_source(&value),
        }
    }
}
