// Only for testing purposes!
// An instruction that outputs consecutive numbers

use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, AnyDestination, Arguments, Destination, DestinationWriter, Register1,
        RelativeStack,
    },
    Instruction, State,
};

static mut N: usize = 0;

fn count<Out: Destination>(state: &mut State, instruction: *const Instruction) {
    instruction_boilerplate(state, instruction, |state, args| {
        let next = unsafe {
            N += 1;
            N
        };
        Out::set(args, state, next.into());
    });
}

impl Instruction {
    pub fn from_counter(out: AnyDestination) -> Self {
        let mut arguments = Arguments::default();
        out.write_destination(&mut arguments);
        Self {
            handler: match out {
                AnyDestination::Register1(_) => count::<Register1>,
                AnyDestination::AbsoluteStack(_) => count::<AbsoluteStack>,
                AnyDestination::RelativeStack(_) => count::<RelativeStack>,
            },
            arguments,
        }
    }
}
