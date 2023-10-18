use crate::{addressing_modes::Arguments, Instruction, State};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    state: &mut State,
    mut instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments),
) {
    unsafe {
        business_logic(state, &(*instruction).arguments);

        loop {
            instruction = instruction.add(1);
            if (*instruction).arguments.predicate.satisfied(&state.flags) {
                break;
            }
        }

        ((*instruction).handler)(state, instruction)
    }
}
