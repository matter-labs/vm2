use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Source},
    decommit::address_into_u256,
    instruction::InstructionResult,
    state::State,
    Instruction, VirtualMachine, World,
};
use u256::U256;
use zkevm_opcode_defs::{Opcode, VmMetaParameters};

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

fn set_context_u128(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        let value = Register1::get(args, &mut vm.state).low_u128();
        vm.state.set_context_u128(value);
    })
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

fn aux_mutating(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |_, _, _| {
        // This instruction just crashes or nops
    })
}

impl Instruction {
    fn from_context<Op: ContextOp>(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self {
            opcode,
            handler: context::<Op>,
            arguments: arguments.write_destination(&out),
        }
    }

    pub fn from_this(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<This>(opcode, out, arguments)
    }
    pub fn from_caller(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<Caller>(opcode, out, arguments)
    }
    pub fn from_code_address(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<CodeAddress>(opcode, out, arguments)
    }
    pub fn from_ergs_left(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<ErgsLeft>(opcode, out, arguments)
    }
    pub fn from_context_u128(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<U128>(opcode, out, arguments)
    }
    pub fn from_context_sp(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self::from_context::<SP>(opcode, out, arguments)
    }
    pub fn from_context_meta(opcode: Opcode, out: Register1, arguments: Arguments) -> Self {
        Self {
            opcode,
            handler: context_meta,
            arguments: arguments.write_destination(&out),
        }
    }
    pub fn from_set_context_u128(opcode: Opcode, src: Register1, arguments: Arguments) -> Self {
        Self {
            opcode,
            handler: set_context_u128,
            arguments: arguments.write_source(&src),
        }
    }
    pub fn from_increment_tx_number(opcode: Opcode, arguments: Arguments) -> Self {
        Self {
            opcode,
            handler: increment_tx_number,
            arguments,
        }
    }
    pub fn from_aux_mutating(opcode: Opcode, arguments: Arguments) -> Self {
        Self {
            opcode,
            handler: aux_mutating,
            arguments,
        }
    }
}
