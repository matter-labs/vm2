use zksync_vm2_interface::{opcodes, Tracer};

use super::{
    common::boilerplate,
    monomorphization::{match_source, monomorphize, parameterize},
};
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Destination,
        Immediate1, Register1, RelativeStack, Source,
    },
    instruction::{ExecutionStatus, Instruction},
    VirtualMachine,
};

fn jump<T: Tracer, W, In: Source>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::Jump, _, _>(vm, world, tracer, |vm, args| {
        #[allow(clippy::cast_possible_truncation)] // intentional
        let target = In::get(args, &mut vm.state).low_u32() as u16;

        let next_instruction = vm.state.current_frame.get_pc_as_u16();
        Register1::set(args, &mut vm.state, next_instruction.into());

        vm.state.current_frame.set_pc_from_u16(target);
    })
}

impl<T: Tracer, W> Instruction<T, W> {
    /// Creates a [`Jump`](opcodes::Jump) instruction with the provided params.
    pub fn from_jump(source: AnySource, destination: Register1, arguments: Arguments) -> Self {
        Self {
            handler: monomorphize!(jump [T W] match_source source),
            arguments: arguments
                .write_source(&source)
                .write_destination(&destination),
        }
    }
}
