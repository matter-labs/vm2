use crate::{addressing_modes::Arguments, instruction::InstructionResult, VirtualMachine, World};
use eravm_stable_interface::Tracer;

#[inline(always)]
pub(crate) fn instruction_boilerplate<Opcode, T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    mut tracer: &mut T,
    business_logic: impl FnOnce(&mut VirtualMachine<T>, &Arguments, &mut dyn World<T>),
) -> InstructionResult {
    Tracer::<Opcode>::before_instruction(&mut tracer, vm);
    let result = unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);
        business_logic(vm, &(*instruction).arguments, world);
        None
    };
    Tracer::<Opcode>::after_instruction(&mut tracer, vm);

    result
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext<Opcode, T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    mut tracer: &mut T,
    business_logic: impl FnOnce(
        &mut VirtualMachine<T>,
        &Arguments,
        &mut dyn World<T>,
    ) -> InstructionResult,
) -> InstructionResult {
    Tracer::<Opcode>::before_instruction(&mut tracer, vm);

    let result = unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);

        business_logic(vm, &(*instruction).arguments, world)
    };
    Tracer::<Opcode>::after_instruction(&mut tracer, vm);

    result
}
