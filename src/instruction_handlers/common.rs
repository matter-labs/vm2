use crate::{addressing_modes::Arguments, Instruction, State};

#[inline(always)]
pub(crate) fn instruction_boilerplate(
    state: &mut State,
    mut instruction: *const Instruction,
    business_logic: impl FnOnce(&mut State, &Arguments),
) {
    let args = unsafe { &(*instruction).arguments };
    unsafe {
        instruction = instruction.add(1);
    };

    if args.predicate.satisfied(&state.flags) {
        business_logic(state, args);
    }

    unsafe { ((*instruction).handler)(state, instruction) }
}
