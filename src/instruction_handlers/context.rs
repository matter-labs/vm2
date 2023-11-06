use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, DestinationWriter, Register1},
    decommit::address_into_u256,
    state::ExecutionResult,
    Instruction, State,
};
use u256::U256;

fn context<Op: ContextOp>(state: &mut State, instruction: *const Instruction) -> ExecutionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        Register1::set(args, state, Op::get(state))
    })
}

trait ContextOp {
    fn get(state: &State) -> U256;
}

struct This;
impl ContextOp for This {
    fn get(state: &State) -> U256 {
        address_into_u256(state.current_frame.address)
    }
}

struct Caller;
impl ContextOp for Caller {
    fn get(state: &State) -> U256 {
        address_into_u256(state.current_frame.caller)
    }
}

struct CodeAddress;
impl ContextOp for CodeAddress {
    fn get(state: &State) -> U256 {
        address_into_u256(state.current_frame.code_address)
    }
}

struct ErgsLeft;
impl ContextOp for ErgsLeft {
    fn get(state: &State) -> U256 {
        U256([state.current_frame.gas as u64, 0, 0, 0])
    }
}

impl Instruction {
    fn from_context<Op: ContextOp>(out: Register1) -> Self {
        let mut arguments = Arguments::default();
        out.write_destination(&mut arguments);

        Self {
            handler: context::<Op>,
            arguments,
        }
    }

    pub fn from_this(out: Register1) -> Self {
        Self::from_context::<This>(out)
    }
    pub fn from_caller(out: Register1) -> Self {
        Self::from_context::<Caller>(out)
    }
    pub fn from_code_address(out: Register1) -> Self {
        Self::from_context::<CodeAddress>(out)
    }
    pub fn from_ergs_left(out: Register1) -> Self {
        Self::from_context::<ErgsLeft>(out)
    }
}
