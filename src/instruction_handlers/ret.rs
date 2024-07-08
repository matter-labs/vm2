use super::{far_call::get_far_call_calldata, HeapInterface};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source, INVALID_INSTRUCTION_COST},
    callframe::FrameRemnant,
    instruction::{ExecutionEnd, InstructionResult},
    mode_requirements::ModeRequirements,
    predication::Flags,
    Instruction, Predicate, VirtualMachine, World,
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
    _: &mut dyn World,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let mut return_type = ReturnType::from_u8(RETURN_TYPE);
    let near_call_leftover_gas = vm.state.current_frame.gas;

    let (pc, snapshot, leftover_gas) = if let Some(FrameRemnant {
        program_counter,
        exception_handler,
        snapshot,
    }) = vm.state.current_frame.pop_near_call()
    {
        (
            if TO_LABEL {
                Immediate1::get(args, &mut vm.state).low_u32() as u16
            } else if return_type.is_failure() {
                exception_handler
            } else {
                program_counter.wrapping_add(1)
            },
            snapshot,
            near_call_leftover_gas,
        )
    } else {
        let return_value_or_panic = if return_type == ReturnType::Panic {
            None
        } else {
            let (raw_abi, is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);
            let result = get_far_call_calldata(raw_abi, is_pointer, vm, false).filter(|pointer| {
                vm.state.current_frame.is_kernel
                    || pointer.memory_page != vm.state.current_frame.calldata_heap
            });

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

        let Some(FrameRemnant {
            program_counter,
            exception_handler,
            snapshot,
        }) = vm.pop_frame(
            return_value_or_panic
                .as_ref()
                .map(|pointer| pointer.memory_page),
        )
        else {
            // The initial frame is not rolled back, even if it fails.
            // It is the caller's job to clean up when the execution as a whole fails because
            // the caller may take external snapshots while the VM is in the initial frame and
            // these would break were the initial frame to be rolled back.

            return if let Some(return_value) = return_value_or_panic {
                let output = vm.state.heaps[return_value.memory_page]
                    .read_range(return_value.start, return_value.length);
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
                exception_handler
            } else {
                program_counter.wrapping_add(1)
            },
            snapshot,
            leftover_gas,
        )
    };

    if return_type.is_failure() {
        vm.world_diff.rollback(snapshot);
    }

    vm.state.flags = Flags::new(return_type == ReturnType::Panic, false, false);
    vm.state.current_frame.gas += leftover_gas;

    match vm.state.current_frame.pc_from_u16(pc) {
        Some(i) => Ok(i),
        None => Ok(&INVALID_INSTRUCTION),
    }
}

/// Formally, a far call pushes a new frame and returns from it immediately if it panics.
/// This function instead panics without popping a frame to save on allocation.
/// TODO: when tracers are implemented, this function should count as a separate instruction!
pub(crate) fn panic_from_failed_far_call(
    vm: &mut VirtualMachine,
    exception_handler: u16,
) -> InstructionResult {
    // Gas is already subtracted in the far call code.
    // No need to roll back, as no changes are made in this "frame".

    vm.state.set_context_u128(0);

    vm.state.registers = [U256::zero(); 16];
    vm.state.register_pointer_flags = 2;

    vm.state.flags = Flags::new(true, false, false);

    match vm.state.current_frame.pc_from_u16(exception_handler) {
        Some(i) => Ok(i),
        None => Ok(&INVALID_INSTRUCTION),
    }
}

/// Panics, burning all available gas.
pub const INVALID_INSTRUCTION: Instruction = Instruction {
    handler: ret::<{ ReturnType::Panic as u8 }, false>,
    arguments: Arguments::new(
        Predicate::Always,
        INVALID_INSTRUCTION_COST,
        ModeRequirements::none(),
    ),
};

pub(crate) const RETURN_COST: u32 = 5;
pub static PANIC: Instruction = Instruction {
    handler: ret::<{ ReturnType::Panic as u8 }, false>,
    arguments: Arguments::new(Predicate::Always, RETURN_COST, ModeRequirements::none()),
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
pub(crate) fn free_panic(vm: &mut VirtualMachine, world: &mut dyn World) -> InstructionResult {
    ret::<{ ReturnType::Panic as u8 }, false>(vm, &PANIC, world)
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_ret(src1: Register1, label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Normal as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: arguments.write_source(&src1).write_source(&label),
        }
    }
    pub fn from_revert(src1: Register1, label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Revert as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: arguments.write_source(&src1).write_source(&label),
        }
    }
    pub fn from_panic(label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        const RETURN_TYPE: u8 = ReturnType::Panic as u8;
        Self {
            handler: monomorphize!(ret [RETURN_TYPE] match_boolean to_label),
            arguments: arguments.write_source(&label),
        }
    }

    pub fn from_invalid() -> Self {
        INVALID_INSTRUCTION
    }
}
