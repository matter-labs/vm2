use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        destination_stack_address, AdvanceStackPointer, Arguments, DestinationWriter, Source,
        SourceWriter,
    },
    Instruction, State, World,
};

fn nop<W: World>(state: &mut State<W>, instruction: *const Instruction<W>) {
    instruction_boilerplate(state, instruction, |state, args| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, state);
        state.current_frame.sp = state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, state));
    });
}

impl<W: World> Instruction<W> {
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
