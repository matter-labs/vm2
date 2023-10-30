use super::{
    common::{instruction_boilerplate, run_next_instruction},
    ret,
};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Register1, Register2, Source, SourceWriter,
    },
    Instruction, Predicate, State, World,
};

fn sstore<W: World>(state: &mut State<W>, instruction: *const Instruction<W>) {
    let args = unsafe { &(*instruction).arguments };

    let key = Register1::get(args, state);
    let value = Register2::get(args, state);

    if state.use_gas(1) {
        return ret::panic();
    }

    state
        .world
        .write_storage(state.current_frame.address, key, value);

    run_next_instruction(state, instruction);
}

fn sload<W: World>(state: &mut State<W>, instruction: *const Instruction<W>) {
    instruction_boilerplate(state, instruction, |state, args| {
        let key = Register1::get(args, state);
        let value = state.world.read_storage(state.current_frame.address, key);
        Register1::set(args, state, value);
    })
}

impl<W: World> Instruction<W> {
    #[inline(always)]
    pub fn from_sstore(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        src1.write_source(&mut arguments);
        src2.write_source(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: sstore::<W>,
            arguments,
        }
    }
}

impl<W: World> Instruction<W> {
    #[inline(always)]
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        src.write_source(&mut arguments);
        dst.write_destination(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: sload::<W>,
            arguments,
        }
    }
}
