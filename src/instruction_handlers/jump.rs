use super::ret::INVALID_INSTRUCTION;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source,
    },
    instruction::{Instruction, InstructionResult},
    VirtualMachine,
};

fn jump<In: Source>(
    vm: &mut VirtualMachine,
    mut instruction: *const Instruction,
) -> InstructionResult {
    unsafe {
        let target = In::get(&(*instruction).arguments, &mut vm.state).low_u32() as u16 as usize;
        if let Some(i) = vm.state.current_frame.program.instructions().get(target) {
            instruction = i;
        } else {
            return Ok(&INVALID_INSTRUCTION);
        }

        Ok(instruction)
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_jump(source: AnySource, arguments: Arguments) -> Self {
        Self {
            handler: monomorphize!(jump match_source source),
            arguments: arguments.write_source(&source),
        }
    }
}
