use super::ret_panic;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source,
    },
    predication::Predicate,
    state::{Instruction, InstructionResult, Panic, State},
};

fn jump<In: Source>(state: &mut State, mut instruction: *const Instruction) -> InstructionResult {
    unsafe {
        let target = In::get(&(*instruction).arguments, state).low_u32() as u16 as usize;
        if let Some(i) = state.current_frame.program.get(target) {
            instruction = i;
        } else {
            return ret_panic(state, Panic::InvalidInstruction);
        }

        Ok(instruction)
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_jump(source: AnySource, predicate: Predicate) -> Self {
        Self {
            handler: monomorphize!(jump match_source source),
            arguments: Arguments::new(predicate, 6).write_source(&source),
        }
    }
}
