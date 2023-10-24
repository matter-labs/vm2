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
            return ret::panic();
        }

        if state.use_gas(1) {
            return ret::panic();
        }

        while !(*instruction).arguments.predicate.satisfied(&state.flags) {
            instruction = instruction.add(1);
            if state.use_gas(1) {
                return ret::panic();
            }
        }

        ((*instruction).handler)(state, instruction)
    }
}

use super::{monomorphization::*, ret};

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
