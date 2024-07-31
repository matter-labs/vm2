use crate::{addressing_modes::Arguments, instruction::InstructionResult, VirtualMachine, World};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    vm: &mut VirtualMachine,
    world: &mut dyn World,
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments, &mut dyn World),
) -> InstructionResult {
    unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);
        business_logic(vm, &(*instruction).arguments, world);
        None
    }
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_ext(
    vm: &mut VirtualMachine,
    world: &mut dyn World,
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments, &mut dyn World) -> InstructionResult,
) -> InstructionResult {
    unsafe {
        let instruction = vm.state.current_frame.pc;
        vm.state.current_frame.pc = instruction.add(1);

        business_logic(vm, &(*instruction).arguments, world)
    }
}
