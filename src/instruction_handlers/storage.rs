use super::common::{instruction_boilerplate, run_next_instruction};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Register1, Register2, Source, SourceWriter,
    },
    state::ExecutionResult,
    Instruction, Predicate, State, World,
};

fn sstore(state: &mut State, instruction: *const Instruction) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    let key = Register1::get(args, state);
    let value = Register2::get(args, state);

    state.use_gas(1)?;

    state
        .world
        .write_storage(state.current_frame.address, key, value);

    run_next_instruction(state, instruction)
}

fn sload(state: &mut State, instruction: *const Instruction) -> ExecutionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        let key = Register1::get(args, state);
        let value = state.world.read_storage(state.current_frame.address, key);
        Register1::set(args, state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        src1.write_source(&mut arguments);
        src2.write_source(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: sstore,
            arguments,
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        src.write_source(&mut arguments);
        dst.write_destination(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: sload,
            arguments,
        }
    }
}
