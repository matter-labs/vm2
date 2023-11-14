use crate::{
    addressing_modes::Arguments,
    state::{ExecutionEnd, InstructionResult},
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
    business_logic: impl FnOnce(&mut State, &Arguments) -> Result<(), ExecutionEnd>,
) -> InstructionResult {
    unsafe {
        business_logic(state, &(*instruction).arguments)?;
        Ok(instruction.add(1))
    }
}
