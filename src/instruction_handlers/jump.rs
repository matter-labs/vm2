use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Destination,
        Immediate1, Register1, RelativeStack, Source,
    },
    instruction::{ExecutionStatus, Instruction},
    VirtualMachine, World,
};
use eravm_stable_interface::opcodes;

fn jump<T, In: Source>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<opcodes::Jump, _>(vm, world, tracer, |vm, args, _| {
        let target = In::get(args, &mut vm.state).low_u32() as u16;

        let next_instruction = vm.state.current_frame.get_pc_as_u16();
        Register1::set(args, &mut vm.state, next_instruction.into());

        vm.state.current_frame.set_pc_from_u16(target);
    })
}

use super::monomorphization::*;

impl<T> Instruction<T> {
    pub fn from_jump(source: AnySource, destination: Register1, arguments: Arguments) -> Self {
        Self {
            handler: monomorphize!(jump [T] match_source source),
            arguments: arguments
                .write_source(&source)
                .write_destination(&destination),
        }
    }
}
