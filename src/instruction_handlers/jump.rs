use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source, SourceWriter,
    },
    predication::Predicate,
    state::{Instruction, State},
};

fn jump<In: Source>(state: &mut State, mut instruction: *const Instruction) {
    unsafe {
        let target = In::get(&(*instruction).arguments, state).low_u32() as u16 as usize;
        if target < state.current_frame.program_len {
            instruction = state.current_frame.program_start.add(target);
        } else {
            // TODO panic
            return;
        }

        while !(*instruction).arguments.predicate.satisfied(&state.flags) {
            instruction = instruction.add(1);
        }

        ((*instruction).handler)(state, instruction)
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_jump(source: AnySource, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        source.write_source(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: monomorphize!(jump match_source source),
            arguments,
        }
    }
}
