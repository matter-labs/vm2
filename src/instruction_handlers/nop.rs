use zkevm_opcode_defs::Opcode;

use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{destination_stack_address, AdvanceStackPointer, Arguments, Source},
    instruction::InstructionResult,
    Instruction, VirtualMachine, World,
};

fn nop(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, &mut vm.state);
        vm.state.current_frame.sp = vm
            .state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, &mut vm.state));
    })
}

impl Instruction {
    pub fn from_nop(
        opcode: Opcode,
        pop: AdvanceStackPointer,
        push: AdvanceStackPointer,
        arguments: Arguments,
    ) -> Self {
        Self {
            opcode,
            handler: nop,
            arguments: arguments.write_source(&pop).write_destination(&push),
        }
    }
}
