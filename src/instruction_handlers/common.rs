use super::free_panic;
use crate::{addressing_modes::Arguments, instruction::ExecutionStatus, VirtualMachine};
use eravm_stable_interface::{opcodes, OpcodeType, Tracer};

#[inline(always)]
pub(crate) fn instruction_boilerplate<Opcode: OpcodeType, T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T, W>, &Arguments, &mut W),
) -> ExecutionStatus {
    instruction_boilerplate_ext::<Opcode, T, W>(vm, world, tracer, |vm, args, _, world| {
        business_logic(vm, args, world);
        ExecutionStatus::Running
    })
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext<Opcode: OpcodeType, T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
    business_logic: impl FnOnce(
        &mut VirtualMachine<T, W>,
        &Arguments,
        &mut T,
        &mut W,
    ) -> ExecutionStatus,
) -> ExecutionStatus {
    let args = unsafe { &(*vm.state.current_frame.pc).arguments };

    if vm.state.use_gas(args.get_static_gas_cost()).is_err()
        || !args.mode_requirements().met(
            vm.state.current_frame.is_kernel,
            vm.state.current_frame.is_static,
        )
    {
        return free_panic(vm, tracer);
    }

    if args.predicate().satisfied(&vm.state.flags) {
        tracer.before_instruction::<Opcode, _>(vm);
        vm.state.current_frame.pc = unsafe { vm.state.current_frame.pc.add(1) };
        let result = business_logic(vm, args, tracer, world);
        tracer.after_instruction::<Opcode, _>(vm);
        result
    } else {
        tracer.before_instruction::<opcodes::Nop, _>(vm);
        vm.state.current_frame.pc = unsafe { vm.state.current_frame.pc.add(1) };
        tracer.after_instruction::<opcodes::Nop, _>(vm);
        ExecutionStatus::Running
    }
}
