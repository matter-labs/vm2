use super::ret_panic;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source,
    },
    instruction::{Instruction, InstructionResult, Panic},
    predication::Predicate,
    VirtualMachine,
};

fn jump<In: Source>(
    vm: &mut VirtualMachine,
    mut instruction: *const Instruction,
) -> InstructionResult {
    unsafe {
        let target = In::get(&(*instruction).arguments, &mut vm.state).low_u32() as u16 as usize;
        if let Some(i) = vm.state.current_frame.program.get(target) {
            instruction = i;
        } else {
            return ret_panic(vm, Panic::InvalidInstruction);
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
