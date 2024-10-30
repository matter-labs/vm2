use primitive_types::U256;
use zksync_vm2_interface::{
    opcodes::{self, Normal, Panic, Revert, TypeLevelReturnType},
    ReturnType, Tracer,
};

use super::{
    common::full_boilerplate,
    far_call::get_far_call_calldata,
    monomorphization::{match_boolean, monomorphize, parameterize},
};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source, INVALID_INSTRUCTION_COST},
    callframe::FrameRemnant,
    instruction::{ExecutionEnd, ExecutionStatus},
    mode_requirements::ModeRequirements,
    predication::Flags,
    tracing::VmAndWorld,
    Instruction, Predicate, VirtualMachine, World,
};

fn naked_ret<T: Tracer, W: World<T>, RT: TypeLevelReturnType, const TO_LABEL: bool>(
    vm: &mut VirtualMachine<T, W>,
    args: &Arguments,
) {
    let mut return_type = RT::VALUE;
    let near_call_leftover_gas = vm.state.current_frame.gas;

    let (snapshot, leftover_gas) = if let Some(FrameRemnant {
        exception_handler,
        snapshot,
    }) = vm.state.current_frame.pop_near_call()
    {
        if TO_LABEL {
            let pc = Immediate1::get_u16(args);
            vm.state.current_frame.set_pc_from_u16(pc);
        } else if return_type.is_failure() {
            vm.state.current_frame.set_pc_from_u16(exception_handler);
        }

        (snapshot, near_call_leftover_gas)
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

            // But to continue execution would be nonsensical and can cause UB because there
            // is no next instruction after a panic arising from some other instruction.
            vm.state.current_frame.pc = invalid_instruction();

            vm.state.status = if let Some(return_value) = return_value_or_panic {
                let output = vm.state.heaps[return_value.memory_page]
                    .read_range_big_endian(
                        return_value.start..return_value.start + return_value.length,
                    )
                    .clone();
                if return_type == ReturnType::Revert {
                    ExecutionStatus::Stopped(ExecutionEnd::Reverted(output))
                } else {
                    ExecutionStatus::Stopped(ExecutionEnd::ProgramFinished(output))
                }
            } else {
                ExecutionStatus::Stopped(ExecutionEnd::Panicked)
            };
            return;
        };

        vm.state.set_context_u128(0);
        vm.state.registers = [U256::zero(); 16];

        if let Some(return_value) = return_value_or_panic {
            vm.state.registers[1] = return_value.into_u256();
        }
        vm.state.register_pointer_flags = 2;

        if return_type.is_failure() {
            vm.state.current_frame.set_pc_from_u16(exception_handler);
        }

        (snapshot, leftover_gas)
    };

    if return_type.is_failure() {
        vm.world_diff.rollback(snapshot);
    }

    vm.state.flags = Flags::new(return_type == ReturnType::Panic, false, false);
    vm.state.current_frame.gas += leftover_gas;
}

fn ret<T: Tracer, W: World<T>, RT: TypeLevelReturnType, const TO_LABEL: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) {
    full_boilerplate::<opcodes::Ret<RT>, _, _>(vm, world, tracer, |vm, args, _, _| {
        naked_ret::<T, W, RT, TO_LABEL>(vm, args);
    });
}

/// Turn the current instruction into a panic at no extra cost. (Great value, I know.)
///
/// Call this when:
/// - gas runs out when paying for the fixed cost of an instruction
/// - causing side effects in a static context
/// - using privileged instructions while not in a system call
/// - the far call stack overflows
///
/// For all other panics, point the instruction pointer at [PANIC] instead.
pub(crate) fn free_panic<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) {
    tracer.before_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world });
    // args aren't used for panics unless TO_LABEL
    naked_ret::<T, W, Panic, false>(
        vm,
        &Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    );
    tracer.after_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world });
}

/// Formally, a far call pushes a new frame and returns from it immediately if it panics.
/// This function instead panics without popping a frame to save on allocation.
pub(crate) fn panic_from_failed_far_call<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
    exception_handler: u16,
) {
    tracer.before_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world });

    // Gas is already subtracted in the far call code.
    // No need to roll back, as no changes are made in this "frame".
    vm.state.set_context_u128(0);
    vm.state.registers = [U256::zero(); 16];
    vm.state.register_pointer_flags = 2;
    vm.state.flags = Flags::new(true, false, false);
    vm.state.current_frame.set_pc_from_u16(exception_handler);

    tracer.after_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world });
}

fn invalid<T: Tracer, W: World<T>>(vm: &mut VirtualMachine<T, W>, world: &mut W, tracer: &mut T) {
    vm.state.current_frame.gas = 0;
    free_panic(vm, world, tracer);
}

trait GenericStatics<T, W> {
    const PANIC: Instruction<T, W>;
    const INVALID: Instruction<T, W>;
}

impl<T: Tracer, W: World<T>> GenericStatics<T, W> for () {
    const PANIC: Instruction<T, W> = Instruction {
        handler: ret::<T, W, Panic, false>,
        arguments: Arguments::new(Predicate::Always, RETURN_COST, ModeRequirements::none()),
    };
    const INVALID: Instruction<T, W> = Instruction::from_invalid();
}

// The following functions return references that live for 'static.
// They aren't marked as such because returning any lifetime is more ergonomic.

/// Point the program counter at this instruction when a panic occurs during the logic of and instruction.
pub(crate) fn spontaneous_panic<'a, T: Tracer, W: World<T>>() -> &'a Instruction<T, W> {
    &<()>::PANIC
}

/// Panics, burning all available gas.
pub(crate) fn invalid_instruction<'a, T: Tracer, W: World<T>>() -> &'a Instruction<T, W> {
    &<()>::INVALID
}

pub(crate) const RETURN_COST: u32 = 5;

/// Variations of [`Ret`](opcodes::Ret) instructions.
impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a normal [`Ret`](opcodes::Ret) instruction with the provided params.
    pub fn from_ret(src1: Register1, label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [T W Normal] match_boolean to_label),
            arguments: arguments.write_source(&src1).write_source(&label),
        }
    }

    /// Creates a revert [`Ret`](opcodes::Ret) instruction with the provided params.
    pub fn from_revert(src1: Register1, label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [T W Revert] match_boolean to_label),
            arguments: arguments.write_source(&src1).write_source(&label),
        }
    }

    /// Creates a panic [`Ret`](opcodes::Ret) instruction with the provided params.
    pub fn from_panic(label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [T W Panic] match_boolean to_label),
            arguments: arguments.write_source(&label),
        }
    }

    /// Creates a *invalid* instruction that will panic by draining all gas.
    pub const fn from_invalid() -> Self {
        Self {
            handler: invalid,
            arguments: Arguments::new(
                Predicate::Always,
                INVALID_INSTRUCTION_COST,
                ModeRequirements::none(),
            ),
        }
    }
}
