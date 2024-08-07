use crate::{addressing_modes::Arguments, instruction::ExecutionStatus, VirtualMachine, World};
use eravm_stable_interface::{OpcodeSelect, TracerDispatch};

#[inline(always)]
pub(crate) fn instruction_boilerplate<Opcode, T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T>, &Arguments, &mut dyn World<T>),
) -> ExecutionStatus {
    tracer.opcode::<Opcode>().before_instruction(vm);
    unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);
        business_logic(vm, &(*instruction).arguments, world);
    };
    tracer.opcode::<Opcode>().after_instruction(vm);

    ExecutionStatus::Running
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext<Opcode, T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
    business_logic: impl FnOnce(
        &mut VirtualMachine<T>,
        &Arguments,
        &mut dyn World<T>,
    ) -> ExecutionStatus,
) -> ExecutionStatus {
    tracer.opcode::<Opcode>().before_instruction(vm);
    let result = unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);

        business_logic(vm, &(*instruction).arguments, world)
    };
    tracer.opcode::<Opcode>().after_instruction(vm);

    result
}
