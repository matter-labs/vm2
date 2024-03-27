use crate::{
    addressing_modes::Arguments, instruction::InstructionResult, Instruction, VirtualMachine,
};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments),
) -> InstructionResult {
    unsafe {
        business_logic(vm, &(*instruction).arguments);
        Ok(instruction.add(1))
    }
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_with_panic(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments, InstructionResult) -> InstructionResult,
) -> InstructionResult {
    unsafe { business_logic(vm, &(*instruction).arguments, Ok(instruction.add(1))) }
}
