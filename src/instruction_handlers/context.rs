use super::common::boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Source},
    decommit::address_into_u256,
    instruction::ExecutionStatus,
    state::State,
    Instruction, VirtualMachine,
};
use eravm_stable_interface::{
    opcodes::{self, Caller, CodeAddress, ContextU128, ErgsLeft, This, SP},
    OpcodeType, Tracer,
};
use u256::U256;
use zkevm_opcode_defs::VmMetaParameters;

fn context<T: Tracer, W, Op: ContextOp>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<Op, _, _>(vm, world, tracer, |vm, args| {
        let result = Op::get(&vm.state);
        Register1::set(args, &mut vm.state, result)
    })
}

trait ContextOp: OpcodeType {
    fn get<T, W>(state: &State<T, W>) -> U256;
}

impl ContextOp for This {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        address_into_u256(state.current_frame.address)
    }
}

impl ContextOp for Caller {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        address_into_u256(state.current_frame.caller)
    }
}

impl ContextOp for CodeAddress {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        address_into_u256(state.current_frame.code_address)
    }
}

impl ContextOp for ErgsLeft {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        U256([state.current_frame.gas as u64, 0, 0, 0])
    }
}

impl ContextOp for ContextU128 {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        state.get_context_u128().into()
    }
}

impl ContextOp for SP {
    fn get<T, W>(state: &State<T, W>) -> U256 {
        state.current_frame.sp.into()
    }
}

fn context_meta<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::ContextMeta, _, _>(vm, world, tracer, |vm, args| {
        let result = VmMetaParameters {
            heap_size: vm.state.current_frame.heap_size,
            aux_heap_size: vm.state.current_frame.aux_heap_size,
            this_shard_id: 0, // TODO properly implement shards
            caller_shard_id: 0,
            code_shard_id: 0,
            // This field is actually pubdata!
            aux_field_0: if vm.state.current_frame.is_kernel {
                vm.world_diff.pubdata.0 as u32
            } else {
                0
            },
        }
        .to_u256();

        Register1::set(args, &mut vm.state, result);
    })
}

fn set_context_u128<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::SetContextU128, _, _>(vm, world, tracer, |vm, args| {
        let value = Register1::get(args, &mut vm.state).low_u128();
        vm.state.set_context_u128(value);
    })
}

fn increment_tx_number<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::IncrementTxNumber, _, _>(vm, world, tracer, |vm, _| {
        vm.start_new_tx();
    })
}

fn aux_mutating<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::AuxMutating0, _, _>(vm, world, tracer, |_, _| {
        // This instruction just crashes or nops
    })
}

impl<T: Tracer, W> Instruction<T, W> {
    fn from_context<Op: ContextOp>(out: Register1, arguments: Arguments) -> Self {
        Self {
            handler: context::<T, W, Op>,
            arguments: arguments.write_destination(&out),
        }
    }

    pub fn from_this(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<This>(out, arguments)
    }
    pub fn from_caller(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<Caller>(out, arguments)
    }
    pub fn from_code_address(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<CodeAddress>(out, arguments)
    }
    pub fn from_ergs_left(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<ErgsLeft>(out, arguments)
    }
    pub fn from_context_u128(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<ContextU128>(out, arguments)
    }
    pub fn from_context_sp(out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<SP>(out, arguments)
    }
    pub fn from_context_meta(out: Register1, arguments: Arguments) -> Self {
        Self {
            handler: context_meta,
            arguments: arguments.write_destination(&out),
        }
    }
    pub fn from_set_context_u128(src: Register1, arguments: Arguments) -> Self {
        Self {
            handler: set_context_u128,
            arguments: arguments.write_source(&src),
        }
    }
    pub fn from_increment_tx_number(arguments: Arguments) -> Self {
        Self {
            handler: increment_tx_number,
            arguments,
        }
    }
    pub fn from_aux_mutating(arguments: Arguments) -> Self {
        Self {
            handler: aux_mutating,
            arguments,
        }
    }
}
