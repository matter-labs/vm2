use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Destination,
        Immediate1, Register1, RelativeStack, Source,
    },
    instruction::{Instruction, InstructionResult},
    VirtualMachine, World,
};

fn jump<In: Source>(vm: &mut VirtualMachine, world: &mut dyn World) -> InstructionResult {
    instruction_boilerplate(vm, world, |vm, args, _| {
        let target = In::get(args, &mut vm.state).low_u32() as u16;

        let next_instruction = vm.state.current_frame.get_pc_as_u16();
        Register1::set(args, &mut vm.state, next_instruction.into());

        vm.state.current_frame.set_pc_from_u16(target);
    })
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
