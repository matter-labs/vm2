use super::ret::INVALID_INSTRUCTION;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Destination,
        Immediate1, Register1, RelativeStack, Source,
    },
    instruction::{Instruction, InstructionResult},
    VirtualMachine, World,
};

fn jump<In: Source>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    _: &mut dyn World,
) -> InstructionResult {
    unsafe {
        let args = &(*instruction).arguments;
        let target = In::get(args, &mut vm.state).low_u32() as u16;

        let next_instruction = vm.state.current_frame.pc_to_u16(instruction) + 1;
        Register1::set(args, &mut vm.state, next_instruction.into());

        if let Some(instruction) = vm.state.current_frame.program.instruction(target) {
            Ok(instruction)
        } else {
            Ok(&INVALID_INSTRUCTION)
        }
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_jump(source: AnySource, destination: Register1, arguments: Arguments) -> Self {
        Self {
            handler: monomorphize!(jump match_source source),
            arguments: arguments
                .write_source(&source)
                .write_destination(&destination),
        }
    }
}
