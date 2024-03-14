use super::ret_panic;
use crate::{
    addressing_modes::Arguments,
    instruction::{InstructionResult, Panic},
    Instruction, VirtualMachine,
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
    business_logic: impl FnOnce(&mut VirtualMachine, &Arguments) -> Result<(), Panic>,
) -> InstructionResult {
    unsafe {
        match business_logic(vm, &(*instruction).arguments) {
            Ok(_) => Ok(instruction.add(1)),
            Err(p) => ret_panic(vm, p),
        }
    }
}
