use super::{
    common::{instruction_boilerplate, instruction_boilerplate_with_panic},
    free_panic,
};
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Source},
    decommit::address_into_u256,
    instruction::InstructionResult,
    state::State,
    Instruction, VirtualMachine, World,
};
use u256::U256;
use zkevm_opcode_defs::VmMetaParameters;

fn context<Op: ContextOp>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        let result = Op::get(&vm.state);
        Register1::set(args, &mut vm.state, result)
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

fn context_meta(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        let result = VmMetaParameters {
            heap_size: vm.state.current_frame.heap_size,
            aux_heap_size: vm.state.current_frame.aux_heap_size,
            this_shard_id: 0, // TODO properly implement shards
            caller_shard_id: 0,
            code_shard_id: 0,
            // This field is actually pubdata!
            // TODO PLA-893: This should be zero when not in kernel mode
            aux_field_0: vm.world_diff.pubdata.0 as u32,
        }
        .to_u256();

        Register1::set(args, &mut vm.state, result);
    })
}

fn set_context_u128(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate_with_panic(
        vm,
        instruction,
        world,
        |vm, args, world, continue_normally| {
            if vm.state.current_frame.is_static {
                return free_panic(vm, world);
            }

            let value = Register1::get(args, &mut vm.state).low_u128();
            vm.state.set_context_u128(value);

            continue_normally
        },
    )
}

fn increment_tx_number(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, _, _| {
        vm.start_new_tx();
    })
}

impl Instruction {
    fn from_context<Op: ContextOp>(out: Register1, arguments: Arguments) -> Self {
        Self {
            handler: context::<Op>,
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
}
