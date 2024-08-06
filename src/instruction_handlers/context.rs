use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Source},
    decommit::address_into_u256,
    instruction::InstructionResult,
    state::State,
    Instruction, VirtualMachine, World,
};
use eravm_stable_interface::opcodes::{self, Caller, CodeAddress, ErgsLeft, This, SP, U128};
use u256::U256;
use zkevm_opcode_defs::VmMetaParameters;

fn context<T, Op: ContextOp>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<Op, _>(vm, world, tracer, |vm, args, _| {
        let result = Op::get(&vm.state);
        Register1::set(args, &mut vm.state, result)
    })
}

trait ContextOp {
    fn get<T>(state: &State<T>) -> U256;
}

impl ContextOp for This {
    fn get<T>(state: &State<T>) -> U256 {
        address_into_u256(state.current_frame.address)
    }
}

impl ContextOp for Caller {
    fn get<T>(state: &State<T>) -> U256 {
        address_into_u256(state.current_frame.caller)
    }
}

impl ContextOp for CodeAddress {
    fn get<T>(state: &State<T>) -> U256 {
        address_into_u256(state.current_frame.code_address)
    }
}

impl ContextOp for ErgsLeft {
    fn get<T>(state: &State<T>) -> U256 {
        U256([state.current_frame.gas as u64, 0, 0, 0])
    }
}

impl ContextOp for U128 {
    fn get<T>(state: &State<T>) -> U256 {
        state.get_context_u128().into()
    }
}

impl ContextOp for SP {
    fn get<T>(state: &State<T>) -> U256 {
        state.current_frame.sp.into()
    }
}

fn context_meta<T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<opcodes::ContextMeta, _>(vm, world, tracer, |vm, args, _| {
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

fn set_context_u128<T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<opcodes::SetContextU128, _>(vm, world, tracer, |vm, args, _| {
        let value = Register1::get(args, &mut vm.state).low_u128();
        vm.state.set_context_u128(value);
    })
}

fn increment_tx_number<T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<opcodes::IncrementTxNumber, _>(vm, world, tracer, |vm, _, _| {
        vm.start_new_tx();
    })
}

fn aux_mutating<T>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> InstructionResult {
    instruction_boilerplate::<opcodes::AuxMutating0, _>(vm, world, tracer, |_, _, _| {
        // This instruction just crashes or nops
    })
}

impl<T> Instruction<T> {
    fn from_context<Op: ContextOp>(out: Register1, arguments: Arguments) -> Self {
        Self {
            handler: context::<T, Op>,
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
        Self::from_context::<U128>(out, arguments)
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
