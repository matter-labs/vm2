use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{destination_stack_address, AdvanceStackPointer, Arguments, Source},
    instruction::InstructionResult,
    Instruction, VirtualMachine, World,
};
use eravm_stable_interface::opcodes;

fn nop<T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<opcodes::Nop, _>(vm, world, tracer, |vm, args, _| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, &mut vm.state);
        vm.state.current_frame.sp = vm
            .state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, &mut vm.state));
    })
}

impl<T> Instruction<T> {
    pub fn from_nop(
        pop: AdvanceStackPointer,
        push: AdvanceStackPointer,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: nop,
            arguments: arguments.write_source(&pop).write_destination(&push),
        }
    }
}
