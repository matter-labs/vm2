use crate::{
    addressing_modes::Arguments,
    state::{ExecutionResult, Panic},
    Instruction, State,
};

macro_rules! run_next_instruction {
    ($state: expr, $instruction: expr) => {{
        loop {
            $instruction = $instruction.add(1);
            $state.use_gas(1)?;

            if (*$instruction).arguments.predicate.satisfied(&$state.flags) {
                break;
            }
        }

        ((*$instruction).handler)($state, $instruction)
    }};
}

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    state: &mut State,
    mut instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments),
) -> ExecutionResult {
    unsafe {
        business_logic(state, &(*instruction).arguments);
        run_next_instruction!(state, instruction)
    }
}

#[inline(always)]
pub(crate) fn instruction_boilerplate_with_panic(
    state: &mut State,
    mut instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments) -> Result<(), Panic>,
) -> ExecutionResult {
    unsafe {
        business_logic(state, &(*instruction).arguments)?;
        run_next_instruction!(state, instruction)
    }
}
