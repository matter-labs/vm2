use super::ret_panic;
use crate::{
    addressing_modes::Arguments,
    state::{InstructionResult, Panic},
    Instruction, State,
};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    state: &mut State,
    instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments),
) -> InstructionResult {
    unsafe {
        business_logic(state, &(*instruction).arguments);
        Ok(instruction.add(1))
    }
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_with_panic(
    state: &mut State,
    instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments) -> Result<(), Panic>,
) -> InstructionResult {
    unsafe {
        match business_logic(state, &(*instruction).arguments) {
            Ok(_) => Ok(instruction.add(1)),
            Err(p) => ret_panic(state, p),
        }
    }
}
