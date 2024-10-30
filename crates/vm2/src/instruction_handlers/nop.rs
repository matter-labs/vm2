use zksync_vm2_interface::{opcodes, Tracer};

use super::common::boilerplate;
use crate::{
    addressing_modes::{destination_stack_address, AdvanceStackPointer, Arguments, Source},
    Instruction, VirtualMachine, World,
};

fn nop<T: Tracer, W: World<T>>(vm: &mut VirtualMachine<T, W>, world: &mut W, tracer: &mut T) {
    boilerplate::<opcodes::Nop, _, _>(vm, world, tracer, |vm, args| {
        // nop's addressing modes can move the stack pointer!
        AdvanceStackPointer::get(args, &mut vm.state);
        vm.state.current_frame.sp = vm
            .state
            .current_frame
            .sp
            .wrapping_add(destination_stack_address(args, &mut vm.state));
    });
}

impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a [`Nop`](opcodes::Nop) instruction with the provided params.
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
