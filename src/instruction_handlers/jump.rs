use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source,
    },
    predication::Predicate,
    state::{ExecutionEnd, Instruction, InstructionResult, State},
};

fn jump<In: Source>(state: &mut State, mut instruction: *const Instruction) -> InstructionResult {
    unsafe {
        let target = In::get(&(*instruction).arguments, state).low_u32() as u16 as usize;
        if let Some(i) = state.current_frame.program.get(target) {
            instruction = i;
        } else {
            return Err(ExecutionEnd::JumpingOutOfProgram);
        }

        Ok(instruction)
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_jump(source: AnySource, predicate: Predicate) -> Self {
        Self {
            handler: monomorphize!(jump match_source source),
            arguments: Arguments::new(predicate).write_source(&source),
        }
    }
}
