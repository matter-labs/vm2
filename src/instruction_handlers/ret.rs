use super::call::get_far_call_calldata;
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source, INVALID_INSTRUCTION_COST},
    instruction::{ExecutionEnd, InstructionResult, Panic},
    predication::Flags,
    rollback::Rollback,
    Instruction, Predicate, VirtualMachine,
};
use u256::U256;

fn ret<const IS_REVERT: bool, const TO_LABEL: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_left = vm.state.current_frame.gas;

    let (pc, snapshot) = if let Some((pc, eh, snapshot)) = vm.state.current_frame.pop_near_call() {
        (
            if TO_LABEL {
                Immediate1::get(args, &mut vm.state).low_u32()
            } else if IS_REVERT {
                eh
            } else {
                pc + 1
            },
            snapshot,
        )
    } else {
        let return_value = match get_far_call_calldata(
            Register1::get(args, &mut vm.state),
            Register1::is_fat_pointer(args, &mut vm.state),
            vm,
        ) {
            Ok(pointer) => pointer,
            Err(panic) => return ret_panic(vm, panic),
        };

        // TODO check that the return value resides in this or a newer frame's memory

        let Some((pc, eh, snapshot)) = vm.state.pop_frame() else {
            let output = vm.state.heaps[return_value.memory_page as usize]
                [return_value.start as usize..(return_value.start + return_value.length) as usize]
                .to_vec();
            return if IS_REVERT {
                vm.world
                    .rollback(vm.state.current_frame.world_before_this_frame);
                Err(ExecutionEnd::Reverted(output))
            } else {
                vm.world.delete_history();
                Err(ExecutionEnd::ProgramFinished(output))
            };
        };

        vm.state.set_context_u128(0);

        vm.state.registers = [U256::zero(); 16];
        vm.state.registers[1] = return_value.into_u256();
        vm.state.register_pointer_flags = 2;

        (if IS_REVERT { eh } else { pc + 1 }, snapshot)
    };

    if IS_REVERT {
        vm.world.rollback(snapshot);
    }

    vm.state.flags = Flags::new(false, false, false);
    vm.state.current_frame.gas += gas_left;

    match vm.state.current_frame.pc_from_u32(pc) {
        Some(i) => Ok(i),
        None => ret_panic(vm, Panic::InvalidInstruction),
    }
}

fn explicit_panic<const TO_LABEL: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    let label = if TO_LABEL {
        unsafe { Some(Immediate1::get(&(*instruction).arguments, &mut vm.state).low_u32()) }
    } else {
        None
    };
    panic_impl(vm, Panic::ExplicitPanic, label)
}

fn invalid_instruction(vm: &mut VirtualMachine, _: *const Instruction) -> InstructionResult {
    panic_impl(vm, Panic::InvalidInstruction, None)
}

/// To be used for panics resulting from static calls attempting mutation and
/// trying to do system-only operations while not in a system call.
pub(crate) fn free_panic(vm: &mut VirtualMachine, panic: Panic) -> InstructionResult {
    panic_impl(vm, panic, None)
}

const RETURN_COST: u32 = 5;

pub(crate) fn ret_panic(vm: &mut VirtualMachine, mut panic: Panic) -> InstructionResult {
    if let Err(p) = vm.state.use_gas(RETURN_COST) {
        panic = p;
    }
    panic_impl(vm, panic, None)
}

#[inline(always)]
fn panic_impl(
    vm: &mut VirtualMachine,
    mut panic: Panic,
    mut maybe_label: Option<u32>,
) -> InstructionResult {
    loop {
        let (eh, snapshot) = if let Some((_, eh, snapshot)) = vm.state.current_frame.pop_near_call()
        {
            (
                if let Some(label) = maybe_label {
                    label
                } else {
                    eh
                },
                snapshot,
            )
        } else {
            let Some((_, eh, snapshot)) = vm.state.pop_frame() else {
                vm.world
                    .rollback(vm.state.current_frame.world_before_this_frame);
                return Err(ExecutionEnd::Panicked(panic));
            };
            vm.state.registers[1] = U256::zero();
            vm.state.register_pointer_flags |= 1 << 1;
            (eh, snapshot)
        };

        vm.world.rollback(snapshot);

        vm.state.flags = Flags::new(true, false, false);
        vm.state.set_context_u128(0);

        if let Some(instruction) = vm.state.current_frame.program.get(eh as usize) {
            return Ok(instruction);
        }
        panic = Panic::InvalidInstruction;
        maybe_label = None;

        if let Err(p) = vm.state.use_gas(RETURN_COST) {
            panic = p;
        }
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_ret(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [false] match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_revert(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [true] match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_panic(label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(explicit_panic match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST).write_source(&label),
        }
    }

    pub fn from_invalid() -> Self {
        Self {
            handler: invalid_instruction,
            arguments: Arguments::new(Predicate::Always, INVALID_INSTRUCTION_COST),
        }
    }
}
