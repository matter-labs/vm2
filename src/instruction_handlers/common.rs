use crate::{addressing_modes::Arguments, instruction::ExecutionStatus, VirtualMachine, World};
use eravm_stable_interface::{forall_opcodes, StateInterface, Tracer};

#[inline(always)]
pub(crate) fn instruction_boilerplate<Opcode: NotifyTracer, T: Tracer>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T>, &Arguments, &mut dyn World<T>),
) -> ExecutionStatus {
    Opcode::before(tracer, vm);
    unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);
        business_logic(vm, &(*instruction).arguments, world);
    };
    Opcode::after(tracer, vm);

    ExecutionStatus::Running
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext<Opcode: NotifyTracer, T: Tracer>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
    business_logic: impl FnOnce(
        &mut VirtualMachine<T>,
        &Arguments,
        &mut dyn World<T>,
    ) -> ExecutionStatus,
) -> ExecutionStatus {
    Opcode::before(tracer, vm);
    let result = unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);

        business_logic(vm, &(*instruction).arguments, world)
    };
    Opcode::after(tracer, vm);

    result
}

/// Call the before/after_{opcode} method of the tracer.
/// Implemented for all ZSTs representing opcodes.
pub trait NotifyTracer {
    fn before<S: StateInterface, T: Tracer>(tracer: &mut T, state: &mut S);
    fn after<S: StateInterface, T: Tracer>(tracer: &mut T, state: &mut S);
}

macro_rules! implement_notify_tracer {
    ($opcode:ident, $before_method:ident, $after_method:ident) => {
        impl NotifyTracer for $opcode {
            fn before<S: StateInterface, T: Tracer>(tracer: &mut T, state: &mut S) {
                tracer.$before_method(state)
            }

            fn after<S: StateInterface, T: Tracer>(tracer: &mut T, state: &mut S) {
                tracer.$after_method(state)
            }
        }
    };
}

use eravm_stable_interface::opcodes::*;
forall_opcodes!(implement_notify_tracer);
