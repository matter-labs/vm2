use super::far_call::get_far_call_calldata;
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source, INVALID_INSTRUCTION_COST},
    instruction::{ExecutionEnd, InstructionResult},
    predication::Flags,
    Instruction, Predicate, VirtualMachine,
};
use u256::U256;

#[repr(u8)]
#[derive(PartialEq)]
enum ReturnType {
    Normal = 0,
    Revert,
    Panic,
}

impl ReturnType {
    fn is_failure(&self) -> bool {
        *self != ReturnType::Normal
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => ReturnType::Normal,
            1 => ReturnType::Revert,
            2 => ReturnType::Panic,
            _ => unreachable!(),
        }
    }
}

fn ret<const RETURN_TYPE: u8, const TO_LABEL: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let mut return_type = ReturnType::from_u8(RETURN_TYPE);
    let near_call_leftover_gas = vm.state.current_frame.gas;

    let (pc, snapshot, leftover_gas) = if let Some((pc, eh, snapshot)) =
        vm.state.current_frame.pop_near_call()
    {
        (
            if TO_LABEL {
                Immediate1::get(args, &mut vm.state).low_u32() as u16
            } else if return_type.is_failure() {
                eh
            } else {
                pc.wrapping_add(1)
            },
            snapshot,
            near_call_leftover_gas,
        )
    } else {
        let return_value_or_panic = if return_type == ReturnType::Panic {
            None
        } else {
            let result = get_far_call_calldata(
                Register1::get(args, &mut vm.state),
                Register1::is_fat_pointer(args, &mut vm.state),
                vm,
            )
            .filter(|pointer| pointer.memory_page != vm.state.current_frame.calldata_heap);

            if result.is_none() {
                return_type = ReturnType::Panic;
            }
            result
        };

        let leftover_gas = vm
            .state
            .current_frame
            .gas
            .saturating_sub(vm.state.current_frame.stipend);

        let Some((pc, eh, snapshot)) = vm.state.pop_frame(
            return_value_or_panic
                .as_ref()
                .map(|pointer| pointer.memory_page),
        ) else {
            if return_type.is_failure() {
                vm.world
                    .rollback(vm.state.current_frame.world_before_this_frame);
            }

            return if let Some(return_value) = return_value_or_panic {
                let output = vm.state.heaps[return_value.memory_page][return_value.start as usize
                    ..(return_value.start + return_value.length) as usize]
                    .to_vec();
                if return_type == ReturnType::Revert {
                    Err(ExecutionEnd::Reverted(output))
                } else {
                    Err(ExecutionEnd::ProgramFinished(output))
                }
            } else {
                Err(ExecutionEnd::Panicked)
            };
        };

        vm.state.set_context_u128(0);
        vm.state.registers = [U256::zero(); 16];

        if let Some(return_value) = return_value_or_panic {
            vm.state.registers[1] = return_value.into_u256();
        }
        vm.state.register_pointer_flags = 2;

        (
            if return_type.is_failure() {
                eh
            } else {
                pc.wrapping_add(1)
            },
            snapshot,
            leftover_gas,
        )
    };

    if return_type.is_failure() {
        vm.world.rollback(snapshot);
    }
    vm.state.flags = Flags::new(return_type == ReturnType::Panic, false, false);
    vm.state.current_frame.gas += leftover_gas;

    match vm.state.current_frame.pc_from_u16(pc) {
        Some(i) => Ok(i),
        None => Ok(&INVALID_INSTRUCTION),
    }
}

/// Panics, burning all available gas.
pub const INVALID_INSTRUCTION: Instruction = Instruction {
    handler: ret::<{ ReturnType::Panic as u8 }, false>,
    arguments: Arguments::new(Predicate::Always, INVALID_INSTRUCTION_COST),
};

pub const PANIC: Instruction = Instruction {
    handler: ret::<{ ReturnType::Panic as u8 }, false>,
    arguments: Arguments::new(Predicate::Always, RETURN_COST),
};

/// Turn the current instruction into a panic at no extra cost. (Great value, I know.)
///
/// Call this when:
/// - gas runs out when paying for the fixed cost of an instruction
/// - causing side effects in a static context
/// - using privileged instructions while not in a system call
/// - the far call stack overflows
///
/// For all other panics, point the instruction pointer at [PANIC] instead.
pub(crate) fn free_panic(vm: &mut VirtualMachine) -> InstructionResult {
    ret::<{ ReturnType::Panic as u8 }, false>(vm, &PANIC)
}

const RETURN_COST: u32 = 5;

use super::monomorphization::*;

impl Instruction {
    pub fn from_ret(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Normal as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_revert(src1: Register1, label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Revert as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST)
                .write_source(&src1)
                .write_source(&label),
        }
    }
    pub fn from_panic(label: Option<Immediate1>, predicate: Predicate) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Panic as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: Arguments::new(predicate, RETURN_COST).write_source(&label),
        }
    }

    pub fn from_invalid() -> Self {
        INVALID_INSTRUCTION
    }
}
