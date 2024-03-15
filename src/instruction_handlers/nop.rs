use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{destination_stack_address, AdvanceStackPointer, Arguments, Source},
    instruction::InstructionResult,
    Instruction, Predicate, VirtualMachine,
};

fn nop(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(vm, instruction, |vm, args| {
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
        pop: AdvanceStackPointer,
        push: AdvanceStackPointer,
        predicate: Predicate,
    ) -> Self {
        Self {
            handler: nop,
            arguments: Arguments::new(predicate, 6)
                .write_source(&pop)
                .write_destination(&push),
        }
    }
}
