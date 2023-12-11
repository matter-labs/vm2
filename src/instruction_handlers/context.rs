use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Source},
    decommit::address_into_u256,
    state::InstructionResult,
    Instruction, Predicate, State,
};
use u256::U256;
use zkevm_opcode_defs::VmMetaParameters;

fn context<Op: ContextOp>(state: &mut State, instruction: *const Instruction) -> InstructionResult {
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

struct U128;
impl ContextOp for U128 {
    fn get(state: &State) -> U256 {
        state.get_context_u128().into()
    }
}

struct SP;
impl ContextOp for SP {
    fn get(state: &State) -> U256 {
        state.current_frame.sp.into()
    }
}

struct Meta;
impl ContextOp for Meta {
    fn get(state: &State) -> U256 {
        VmMetaParameters {
            ergs_per_pubdata_byte: 1,
            heap_size: state.heaps[state.current_frame.heap as usize].len() as u32,
            aux_heap_size: state.heaps[state.current_frame.aux_heap as usize].len() as u32,
            this_shard_id: 0, // TODO properly implement shards
            caller_shard_id: 0,
            code_shard_id: 0,
        }
        .to_u256()
    }
}

fn set_context_u128(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(state, instruction, |state, args| {
        let value = Register1::get(args, state).low_u128();
        state.set_context_u128(value)
    })
}

impl Instruction {
    fn from_context<Op: ContextOp>(out: Register1, predicate: Predicate) -> Self {
        Self {
            handler: context::<Op>,
            arguments: Arguments::new(predicate, 5).write_destination(&out),
        }
    }

    pub fn from_this(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<This>(out, predicate)
    }
    pub fn from_caller(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<Caller>(out, predicate)
    }
    pub fn from_code_address(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<CodeAddress>(out, predicate)
    }
    pub fn from_ergs_left(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<ErgsLeft>(out, predicate)
    }
    pub fn from_context_u128(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<U128>(out, predicate)
    }
    pub fn from_context_sp(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<SP>(out, predicate)
    }
    pub fn from_context_meta(out: Register1, predicate: Predicate) -> Self {
        Self::from_context::<Meta>(out, predicate)
    }
    pub fn from_set_context_u128(src: Register1, predicate: Predicate) -> Self {
        Self {
            handler: set_context_u128,
            arguments: Arguments::new(predicate, 5).write_source(&src),
        }
    }
}
