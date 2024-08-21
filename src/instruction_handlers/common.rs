use crate::{addressing_modes::Arguments, instruction::ExecutionStatus, VirtualMachine};
use eravm_stable_interface::{OpcodeType, Tracer};

#[inline(always)]
pub(crate) fn instruction_boilerplate<Opcode: OpcodeType, T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T, W>, &Arguments, &mut W),
) -> ExecutionStatus {
    tracer.before_instruction::<Opcode, _>(vm);
    unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);
        business_logic(vm, &(*instruction).arguments, world);
    };
    tracer.after_instruction::<Opcode, _>(vm);

    ExecutionStatus::Running
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext<Opcode: OpcodeType, T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T, W>, &Arguments, &mut W) -> ExecutionStatus,
) -> ExecutionStatus {
    tracer.before_instruction::<Opcode, _>(vm);
    let result = unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);

        business_logic(vm, &(*instruction).arguments, world)
    };
    tracer.after_instruction::<Opcode, _>(vm);

    result
}
