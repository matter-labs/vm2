use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{destination_stack_address, AdvanceStackPointer, Arguments, Source},
    state::InstructionResult,
    Instruction, Predicate, State,
};

fn nop(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, state);
        state.current_frame.sp = state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, state));
    })
}

impl Instruction {
    pub fn from_nop(
        pop: AdvanceStackPointer,
        push: AdvanceStackPointer,
        predicate: Predicate,
    ) -> Self {
        Self {
            handler: nop,
            arguments: Arguments::new(predicate)
                .write_source(&pop)
                .write_destination(&push),
        }
    }
}
