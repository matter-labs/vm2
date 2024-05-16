use crate::{
    addressing_modes::Arguments, instruction::InstructionResult, Instruction, VirtualMachine, World,
};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments, &mut dyn World),
) -> InstructionResult {
    unsafe {
        business_logic(vm, &(*instruction).arguments, world);
        Ok(instruction.add(1))
    }
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_with_panic(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
    business_logic: impl FnOnce(
        &mut VirtualMachine,
        &Arguments,
        &mut dyn World,
        InstructionResult,
    ) -> InstructionResult,
) -> InstructionResult {
    unsafe { business_logic(vm, &(*instruction).arguments, world, Ok(instruction.add(1))) }
}
