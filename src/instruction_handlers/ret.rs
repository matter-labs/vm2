use super::call::get_far_call_arguments;
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source},
    predication::Flags,
    rollback::Rollback,
    state::{ExecutionEnd, InstructionResult, Panic},
    Instruction, Predicate, State,
};
use u256::U256;

fn ret<const IS_REVERT: bool, const TO_LABEL: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_left = state.current_frame.gas;

    let (pc, snapshot) = if let Some((pc, eh, snapshot)) = state.current_frame.pop_near_call() {
        (
            if TO_LABEL {
                Immediate1::get(args, state).low_u32()
            } else if IS_REVERT {
                eh
            } else {
                pc + 1
            },
            snapshot,
        )
    } else {
        let return_value = match get_far_call_arguments(args, state) {
            Ok(abi) => abi.pointer,
            Err(panic) => return ret_panic(state, panic),
        };

        // TODO check that the return value resides in this or a newer frame's memory

        let Some((pc, eh, snapshot)) = state.pop_frame() else {
            let output = state.heaps[return_value.memory_page as usize][return_value.start as usize..(return_value.start + return_value.length) as usize].to_vec();
            return if IS_REVERT{
                state.world.rollback(state.current_frame.world_before_this_frame);
                Err(ExecutionEnd::Reverted(output))
            } else {
                state.world.delete_history();
                Err(ExecutionEnd::ProgramFinished(output))
            };
        };

        state.set_context_u128(0);

        state.registers = [U256::zero(); 16];
        state.registers[1] = return_value.into_u256();
        state.register_pointer_flags = 2;

        (if IS_REVERT { eh } else { pc + 1 }, snapshot)
    };

    if IS_REVERT {
        state.world.rollback(snapshot);
    }

    state.flags = Flags::new(false, false, false);
    state.current_frame.gas += gas_left;

    match state.current_frame.pc_from_u32(pc) {
        Some(i) => Ok(i),
        None => ret_panic(state, Panic::InvalidInstruction),
    }
}

pub(crate) fn ret_panic(state: &mut State, panic: Panic) -> InstructionResult {
    panic_impl(state, panic, None)
}

fn explicit_panic<const TO_LABEL: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    let label = if TO_LABEL {
        unsafe { Some(Immediate1::get(&(*instruction).arguments, state).low_u32()) }
    } else {
        None
    };
    panic_impl(state, Panic::ExplicitPanic, label)
}

#[inline(always)]
fn panic_impl(
    state: &mut State,
    mut panic: Panic,
    mut maybe_label: Option<u32>,
) -> InstructionResult {
    loop {
        let (eh, snapshot) = if let Some((_, eh, snapshot)) = state.current_frame.pop_near_call() {
            (
                if let Some(label) = maybe_label {
                    label
                } else {
                    eh
                },
                snapshot,
            )
        } else {
            let Some((_, eh, snapshot)) = state.pop_frame() else {
                state.world.rollback(state.current_frame.world_before_this_frame);
                return Err(ExecutionEnd::Panicked(panic));
            };
            state.registers[1] = U256::zero();
            state.register_pointer_flags |= 1 << 1;
            (eh, snapshot)
        };

        state.world.rollback(snapshot);

        state.flags = Flags::new(true, false, false);
        state.set_context_u128(0);

        if let Some(instruction) = state.current_frame.program.get(eh as usize) {
            return Ok(instruction);
        }
        panic = Panic::InvalidInstruction;
        maybe_label = None;
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_ret(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [false] match_boolean to_label),
            arguments: Arguments::new(predicate, 5)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_revert(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [true] match_boolean to_label),
            arguments: Arguments::new(predicate, 5)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_panic(label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(explicit_panic match_boolean to_label),
            arguments: Arguments::new(predicate, 5).write_source(&label),
        }
    }
}
