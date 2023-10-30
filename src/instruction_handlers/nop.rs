use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        destination_stack_address, AdvanceStackPointer, Arguments, DestinationWriter, Source,
        SourceWriter,
    },
    Instruction, State,
};

fn nop(state: &mut State, instruction: *const Instruction) {
    instruction_boilerplate(state, instruction, |state, args| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, state);
        state.current_frame.sp = state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, state));
    });
}

impl Instruction {
    pub fn from_nop(pop: AdvanceStackPointer, push: AdvanceStackPointer) -> Self {
        let mut arguments = Arguments::default();
        pop.write_source(&mut arguments);
        push.write_destination(&mut arguments);

        Self {
            handler: nop,
            arguments,
        }
    }
}
