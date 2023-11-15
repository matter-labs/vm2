use super::common::{instruction_boilerplate, instruction_boilerplate_with_panic};
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source, SSTORE_COST},
    state::InstructionResult,
    Instruction, Predicate, State, World,
};

fn sstore(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
        let args = unsafe { &(*instruction).arguments };

        let key = Register1::get(args, state);
        let value = Register2::get(args, state);

        state.use_gas(1)?;

        state
            .world
            .write_storage(state.current_frame.address, key, value);

        Ok(())
    })
}

fn sload(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        let key = Register1::get(args, state);
        let value = state.world.read_storage(state.current_frame.address, key);
        Register1::set(args, state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        Self {
            handler: sstore,
            arguments: Arguments::new(predicate, SSTORE_COST)
                .write_source(&src1)
                .write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        Self {
            handler: sload,
            arguments: Arguments::new(predicate, 158)
                .write_source(&src)
                .write_destination(&dst),
        }
    }
}
