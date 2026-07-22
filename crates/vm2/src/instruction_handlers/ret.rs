use primitive_types::U256;
use zksync_vm2_interface::{
    opcodes::{self, Normal, Panic, Revert, TypeLevelReturnType},
    ReturnType, Tracer,
};

use super::{
    common::full_boilerplate,
    far_call::get_calldata,
    monomorphization::{match_boolean, monomorphize, parameterize},
};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Source, INVALID_INSTRUCTION_COST},
    callframe::FrameRemnant,
    instruction::{ExecutionEnd, ExecutionStatus},
    mode_requirements::ModeRequirements,
    page_ids::base_page_from_heap,
    predication::Flags,
    tracing::VmAndWorld,
    Instruction, Predicate, VirtualMachine, World,
};

fn naked_ret<T: Tracer, W: World<T>, RT: TypeLevelReturnType, const TO_LABEL: bool>(
    vm: &mut VirtualMachine<T, W>,
    args: &Arguments,
) -> ExecutionStatus {
    let mut return_type = RT::VALUE;
    let near_call_leftover_gas = vm.state.current_frame.gas;

    let (snapshot, leftover_gas) = if let Some(FrameRemnant {
        exception_handler,
        snapshot,
    }) = vm.state.current_frame.pop_near_call()
    {
        if vm.state.aborting {
            // An uncatchable unwind is in progress: this near-call return must not stop at its
            // label or exception handler either (`TO_LABEL`/`return_type` are irrelevant here).
            if vm.state.previous_frames.is_empty() {
                // We're already back in the bootloader's (frame 0's) own near-call context:
                // deliver the panic normally so its handler runs, exactly like the ordinary
                // failure path below. Frame 0 itself is never popped by a near-call return.
                vm.state.aborting = false;
                vm.state.current_frame.set_pc_from_u16(exception_handler);
            } else {
                // Not yet at the bootloader: keep re-panicking through the current frame's
                // near-calls (or, once none remain, its far-frame caller) until the bootloader
                // is reached; see the far-frame branch below for the terminal case.
                vm.state.current_frame.gas = 0;
                vm.state.current_frame.pc = spontaneous_panic();
            }
        } else if TO_LABEL {
            let pc = Immediate1::get_u16(args);
            vm.state.current_frame.set_pc_from_u16(pc);
        } else if return_type.is_failure() {
            vm.state.current_frame.set_pc_from_u16(exception_handler);
        }

        (snapshot, near_call_leftover_gas)
    } else {
        let (raw_abi, is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);
        let return_value_or_panic = if return_type == ReturnType::Panic {
            // A panic forwards no returndata, but the return-ABI pointer must still be resolved for
            // its heap-growth cost: a fresh-heap pointer whose `start + length` overflows `u32`
            // grows the heap to `u32::MAX`, draining the frame's gas. Passing `already_failed = true`
            // charges exactly that penalty while discarding the (unused) returndata pointer. This
            // mirrors the proving circuit and post-#217 zk_evm; see `get_calldata`.
            get_calldata(raw_abi, is_pointer, vm, true);
            None
        } else {
            let result = get_calldata(raw_abi, is_pointer, vm, false).filter(|pointer| {
                if vm.state.current_frame.is_kernel {
                    true
                } else {
                    // Non-kernel returndata forwarding must be unidirectional: callers may pass
                    // pointers down the stack, but callees must not forward pointers to older pages.
                    // This mirrors zk_evm's restriction based on base memory page checks.
                    pointer.memory_page.as_u32() >= base_page_from_heap(vm.state.current_frame.heap)
                        && pointer.memory_page != vm.state.current_frame.calldata_heap
                }
            });

            if result.is_none() {
                return_type = ReturnType::Panic;
            }
            result
        };

        let leftover_gas = vm.state.current_frame.gas;

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

            return if let Some(return_value) = return_value_or_panic {
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
        };

        vm.state.set_context_u128(0);
        vm.state.registers = [U256::zero(); 16];

        if let Some(return_value) = return_value_or_panic {
            vm.state.registers[1] = return_value.into_u256();
        }
        vm.state.register_pointer_flags = 2;

        if vm.state.aborting {
            // Same uncatchable-unwind handling as the near-call branch above.
            if vm.state.previous_frames.is_empty() {
                // We just returned into the bootloader (frame 0): deliver the panic normally so
                // its handler runs. Frame 0 is never popped here (`pop_frame` returned `None`
                // for that case above, taking the early-return branch instead).
                vm.state.aborting = false;
                vm.state.current_frame.set_pc_from_u16(exception_handler);
            } else {
                // Not yet at the bootloader: re-panic this frame too, skipping its handler.
                vm.state.current_frame.gas = 0;
                vm.state.current_frame.pc = spontaneous_panic();
            }
        } else if return_type.is_failure() {
            vm.state.current_frame.set_pc_from_u16(exception_handler);
        }

        (snapshot, leftover_gas)
    };

    if return_type.is_failure() {
        vm.world_diff.append_rollback_logs(&snapshot);
        vm.world_diff.rollback(snapshot);
    }

    vm.state.flags = Flags::new(return_type == ReturnType::Panic, false, false);
    vm.state.current_frame.gas += leftover_gas;

    ExecutionStatus::Running
}

fn ret<T: Tracer, W: World<T>, RT: TypeLevelReturnType, const TO_LABEL: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    full_boilerplate::<opcodes::Ret<RT>, _, _>(vm, world, tracer, |vm, args, _, _| {
        naked_ret::<T, W, RT, TO_LABEL>(vm, args)
    })
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
) -> ExecutionStatus {
    tracer.before_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world });
    // A spontaneous panic has no return ABI: these empty args encode source register r0, so
    // naked_ret's return-ABI resolution reads zero and charges no heap growth. (args are otherwise
    // only consulted for the jump label when TO_LABEL is set, which it isn't here.)
    naked_ret::<T, W, Panic, false>(
        vm,
        &Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    )
    .merge_tracer(tracer.after_instruction::<opcodes::Ret<Panic>, _>(&mut VmAndWorld { vm, world }))
}

fn invalid<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    vm.state.current_frame.gas = 0;
    free_panic(vm, world, tracer)
}

trait GenericStatics<T, W> {
    const PANIC: Instruction<T, W>;
    const INVALID: Instruction<T, W>;
}

impl<T: Tracer, W: World<T>> GenericStatics<T, W> for () {
    const PANIC: Instruction<T, W> = Instruction::from_spontaneous_panic();
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
    ///
    /// `src1` carries the return-ABI register. Even though a panic forwards no returndata, the
    /// register is still resolved for its heap-growth cost (see `naked_ret`).
    pub fn from_panic(src1: Register1, label: Option<Immediate1>, arguments: Arguments) -> Self {
        let to_label = label.is_some();
        Self {
            handler: monomorphize!(ret [T W Panic] match_boolean to_label),
            arguments: arguments.write_source(&src1).write_source(&label),
        }
    }

    /// Creates the instruction that is executed when anonther instruction encounters
    /// an error.
    pub(crate) const fn from_spontaneous_panic() -> Self {
        Self {
            handler: ret::<T, W, Panic, false>,
            arguments: Arguments::new(Predicate::Always, RETURN_COST, ModeRequirements::none()),
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
