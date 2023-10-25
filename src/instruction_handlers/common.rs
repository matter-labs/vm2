use super::ret;
use crate::{addressing_modes::Arguments, Instruction, State, World};

#[inline(always)]
pub(crate) fn instruction_boilerplate<W: World>(
    state: &mut State<W>,
    instruction: *const Instruction<W>,
    business_logic: impl FnOnce(&mut State<W>, &Arguments),
) {
    unsafe {
        business_logic(state, &(*instruction).arguments);
        run_next_instruction(state, instruction)
    }
}

#[inline(always)]
pub(crate) fn run_next_instruction<W: World>(
    state: &mut State<W>,
    mut instruction: *const Instruction<W>,
) {
    unsafe {
        loop {
            instruction = instruction.add(1);
            if state.use_gas(1) {
                return ret::panic();
            }
            if (*instruction).arguments.predicate.satisfied(&state.flags) {
                break;
            }
        }

        ((*instruction).handler)(state, instruction)
    }
}
