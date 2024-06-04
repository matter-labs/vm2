use super::{heap_access::grow_heap, ret::panic_from_failed_far_call, AuxHeap, Heap};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    callframe::address_is_kernel,
    decommit::{is_kernel, u256_into_address},
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    instruction_handlers::free_panic,
    predication::Flags,
    Instruction, VirtualMachine, World,
};
use u256::{H160, U256};
use zkevm_opcode_defs::{
    system_params::{EVM_SIMULATOR_STIPEND, MSG_VALUE_SIMULATOR_ADDITIVE_COST},
    BlobSha256Format, ContractCodeSha256Format, FarCallForwardPageType, VersionedHashHeader,
    VersionedHashLen32, VersionedHashNormalizedPreimage, ADDRESS_MSG_VALUE,
    STIPENDS_AND_EXTRA_COSTS_TABLE,
};

#[repr(u8)]
pub enum CallingMode {
    Normal,
    Delegate,
    Mimic,
}

fn get_stipend_and_extra_cost(address: &H160, is_system_call: bool) -> (u32, u32) {
    let address_bytes = address.as_fixed_bytes();
    let is_kernel = address_bytes[0..18].iter().all(|&el| el == 0u8);

    if is_kernel {
        if is_system_call {
            let address = u16::from_be_bytes([address_bytes[18], address_bytes[19]]);

            STIPENDS_AND_EXTRA_COSTS_TABLE[address as usize]
        } else {
            (0, 0)
        }
    } else {
        (0, 0)
    }
}

fn can_call_evm_simulator(
    abi: &FarCallABI,
    dst_is_kernel: bool,
    mask_to_default_aa: &mut bool,
    buffer: &[u8; 32],
) -> Result<bool, ()> {
    let is_valid_as_blob_hash = BlobSha256Format::is_valid(&buffer);
    if is_valid_as_blob_hash {
        let is_code_at_rest = BlobSha256Format::is_code_at_rest_if_valid(&buffer);
        let is_constructed = BlobSha256Format::is_in_construction_if_valid(&buffer);

        let can_call_at_rest = !abi.is_constructor_call && is_code_at_rest;
        let can_call_by_constructor = abi.is_constructor_call && is_constructed;

        let can_call_code_without_masking = can_call_at_rest || can_call_by_constructor;
        if can_call_code_without_masking == true {
            Ok(true)
        } else {
            // calling mode is unknown, so it's most likely a normal
            // call to contract that is still created
            if dst_is_kernel == false {
                *mask_to_default_aa = true;
                Ok(false)
            } else {
                Err(())
            }
        }
    } else {
        Ok(false)
    }
}

fn can_call_code_without_masking(
    abi: &FarCallABI,
    dst_is_kernel: bool,
    mask_to_default_aa: &mut bool,
    buffer: &[u8; 32],
) -> Result<bool, ()> {
    let is_valid_as_bytecode_hash = ContractCodeSha256Format::is_valid(&buffer);
    if is_valid_as_bytecode_hash {
        let is_code_at_rest = ContractCodeSha256Format::is_code_at_rest_if_valid(&buffer);
        let is_constructed = ContractCodeSha256Format::is_in_construction_if_valid(&buffer);

        let can_call_at_rest = !abi.is_constructor_call && is_code_at_rest;
        let can_call_by_constructor = abi.is_constructor_call && is_constructed;

        let can_call_code_without_masking = can_call_at_rest || can_call_by_constructor;
        if can_call_code_without_masking == true {
            Ok(true)
        } else {
            // calling mode is unknown, so it's most likely a normal
            // call to contract that is still created
            if dst_is_kernel == false {
                *mask_to_default_aa = true;
                Ok(false)
            } else {
                /*
                exceptions.set(
                    FarCallExceptionFlags::CALL_IN_NOW_CONSTRUCTED_SYSTEM_CONTRACT,
                    true,
                );
                */
                Err(())
            }
        }
    } else {
        Ok(false)
    }
}

/// A call to another contract.
///
/// First, the code of the called contract is fetched and a fat pointer is created
/// or and existing one is forwarded. Costs for decommitting and memory growth are paid
/// at this point.
///
/// A new stack frame is pushed. At most 63/64 of the *remaining* gas is passed to the called contract.
///
/// Even though all errors happen before the new stack frame, they cause a panic in the new frame,
/// not in the caller!
fn far_call<const CALLING_MODE: u8, const IS_STATIC: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let address_mask: U256 = U256::MAX >> (256 - 160);

    let (raw_abi, raw_abi_is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);
    let destination_address = Register2::get(args, &mut vm.state) & address_mask;
    let exception_handler = Immediate1::get(args, &mut vm.state).low_u32() as u16;

    let mut abi = get_far_call_arguments(raw_abi);
    let calldata = get_far_call_calldata(raw_abi, raw_abi_is_pointer, vm);
    let dst_is_kernel = is_kernel(destination_address);

    abi.is_system_call &= dst_is_kernel;

    let code_hash = crate::decommit::code_hash(world, u256_into_address(destination_address));

    let mut buffer = [0u8; 32];
    code_hash.to_big_endian(&mut buffer);
    let mut mask_to_default_aa = false;

    let Ok(can_call_code_without_masking) =
        can_call_code_without_masking(&abi, dst_is_kernel, &mut mask_to_default_aa, &buffer)
    else {
        return free_panic(vm, world);
    };

    let Ok(can_call_evm_simulator) =
        can_call_evm_simulator(&abi, dst_is_kernel, &mut mask_to_default_aa, &buffer)
    else {
        return free_panic(vm, world);
    };

    if code_hash.is_zero() {
        if !dst_is_kernel {
            mask_to_default_aa = true;
        } else {
            /*
            exceptions.set(FarCallExceptionFlags::INVALID_CODE_HASH_FORMAT, true);
            */
            return free_panic(vm, world);
        }
    }

    // Only at most one of call modes may be valid.
    assert!(
        [
            mask_to_default_aa,
            can_call_evm_simulator,
            can_call_code_without_masking
        ]
        .iter()
        .filter(|b| **b)
        .count()
            < 2
    );

    let unknown_hash = mask_to_default_aa == false
        && can_call_evm_simulator == false
        && can_call_code_without_masking == false;
    if unknown_hash {
        /* TODO: translate to vm2
        exceptions.set(FarCallExceptionFlags::INVALID_CODE_HASH_FORMAT, true);
        */
        return free_panic(vm, world);
    }

    // now let's check if code format "makes sense"
    if can_call_code_without_masking {
        // masking is not needed
    } else if can_call_evm_simulator {
        // overwrite buffer with evm simulator bytecode hash
        vm.settings.evm_interpreter_code_hash = buffer;
    } else if mask_to_default_aa {
        // overwrite buffer with default AA code hash
        vm.settings.default_aa_code_hash = buffer;
    } else {
        /* TODO
        assert!(exceptions.is_empty() == false);
        */
    }

    // we also use code hash as an exception hatch here
    if abi.forwarding_mode == FarCallForwardPageType::ForwardFatPointer {
        if !raw_abi_is_pointer {
            /*
            exceptions.set(
                FarCallExceptionFlags::INPUT_IS_NOT_POINTER_WHEN_EXPECTED,
                true,
            );
            */
            return free_panic(vm, world);
        }
    } else {
        if raw_abi_is_pointer {
            // there is no reasonable case to try to re-interpret pointer
            // as integer here
            /*
            exceptions.set(
                FarCallExceptionFlags::INPUT_IS_POINTER_WHEN_NOT_EXPECTED,
                true,
            );
            */
            return free_panic(vm, world);
        }
    }

    // validate that fat pointer (one a future one) we formed is somewhat valid
    let validate_as_fresh = abi.forwarding_mode != FarCallForwardPageType::ForwardFatPointer;

    // NOTE: one can not properly address a range [2^32 - 32..2^32] here, but we never care in practice about this case
    // as one can not ever pay to grow memory to such extent

    let pointer_validation_exceptions = abi.memory_quasi_fat_pointer.validate(validate_as_fresh);

    if !pointer_validation_exceptions.is_empty() {
        // pointer is malformed
        /*
        exceptions.set(FarCallExceptionFlags::MALFORMED_ABI_QUASI_POINTER, true);
        */
        return free_panic(vm, world);
    }
    // this captures the case of empty slice
    if abi.memory_quasi_fat_pointer.validate_as_slice() == false {
        /* TODO
        exceptions.set(FarCallExceptionFlags::MALFORMED_ABI_QUASI_POINTER, true);
        */
        return free_panic(vm, world);
    }

    // these modifications we can do already as all pointer formal validity related things are done
    match abi.forwarding_mode {
        FarCallForwardPageType::ForwardFatPointer => {
            // We can formally shrink the pointer
            // If it was malformed then we masked and overflows can not happen
            let new_start = abi
                .memory_quasi_fat_pointer
                .start
                .wrapping_add(abi.memory_quasi_fat_pointer.offset);
            let new_length = abi
                .memory_quasi_fat_pointer
                .length
                .wrapping_sub(abi.memory_quasi_fat_pointer.offset);

            abi.memory_quasi_fat_pointer.start = new_start;
            abi.memory_quasi_fat_pointer.length = new_length;
            abi.memory_quasi_fat_pointer.offset = 0;
        }
        FarCallForwardPageType::UseHeap => {
            todo!()

            /*
            let owned_page = CallStackEntry::<N, E>::heap_page_from_base(current_base_page).0;

            far_call_abi.memory_quasi_fat_pointer.memory_page = owned_page;
            */
        }
        FarCallForwardPageType::UseAuxHeap => {
            todo!()

            /*
            let owned_page = CallStackEntry::<N, E>::aux_heap_page_from_base(current_base_page).0;

            far_call_abi.memory_quasi_fat_pointer.memory_page = owned_page;
            */
        }
    };

    // we mask out fat pointer based on:
    // - invalid code hash format
    // - call yet constructed kernel
    // - not fat pointer when expected
    // - invalid slice structure in ABI
    if
    /* TODO exceptions.is_empty() == */
    false {
        abi.memory_quasi_fat_pointer = FatPointer::empty();
        // even though we will not pay for memory resize,
        // we do not care
    }

    let current_stack_mut = &mut vm.state.current_frame;

    // potentially pay for memory growth
    let memory_growth_in_bytes = match abi.forwarding_mode {
        a @ FarCallForwardPageType::UseHeap | a @ FarCallForwardPageType::UseAuxHeap => {
            // pointer is already validated, so we do not need to check that start + length do not overflow
            let mut upper_bound =
                abi.memory_quasi_fat_pointer.start + abi.memory_quasi_fat_pointer.length;

            let penalize_out_of_bounds_growth = todo!(); /* pointer_validation_exceptions
                                                         .contains(FatPointerValidationException::DEREF_BEYOND_HEAP_RANGE);
                                                                                                 */
            if penalize_out_of_bounds_growth {
                upper_bound = u32::MAX;
            }

            let current_bound = if a == FarCallForwardPageType::UseHeap {
                current_stack_mut.heap_size
            } else if a == FarCallForwardPageType::UseAuxHeap {
                current_stack_mut.aux_heap_size
            } else {
                unreachable!();
            };
            let (mut diff, uf) = upper_bound.overflowing_sub(current_bound);
            if uf {
                // heap bound is already beyond what we pass
                diff = 0u32;
            } else {
                // save new upper bound in context.
                // Note that we are ok so save even penalizing upper bound because we will burn
                // all the ergs in this frame anyway, and no further resizes are possible
                if a == FarCallForwardPageType::UseHeap {
                    current_stack_mut.heap_size = upper_bound;
                } else if a == FarCallForwardPageType::UseAuxHeap {
                    current_stack_mut.aux_heap_size = upper_bound;
                } else {
                    unreachable!();
                }
            }

            diff
        }
        FarCallForwardPageType::ForwardFatPointer => 0u32,
    };

    let decommit_result = vm.world_diff.decommit(
        world,
        destination_address,
        vm.settings.default_aa_code_hash,
        vm.settings.evm_interpreter_code_hash,
        &mut vm.state.current_frame.gas,
        abi.is_constructor_call,
    );

    let mandated_gas = if destination_address == ADDRESS_MSG_VALUE.into() {
        MSG_VALUE_SIMULATOR_ADDITIVE_COST
    } else {
        0
    };

    // mandated gas is passed even if it means transferring more than the 63/64 rule allows
    if let Some(gas_left) = vm.state.current_frame.gas.checked_sub(mandated_gas) {
        vm.state.current_frame.gas = gas_left;
    } else {
        return panic_from_failed_far_call(vm, exception_handler);
    };

    let maximum_gas = vm.state.current_frame.gas / 64 * 63;
    let new_frame_gas = abi.gas_to_pass.min(maximum_gas);
    vm.state.current_frame.gas -= new_frame_gas;

    let new_frame_gas = new_frame_gas + mandated_gas;

    let (Some(calldata), Some((program, is_evm_interpreter))) = (calldata, decommit_result) else {
        return panic_from_failed_far_call(vm, exception_handler);
    };

    let (mut stipend, _extra_ergs_from_caller_to_callee) =
        get_stipend_and_extra_cost(&u256_into_address(destination_address), abi.is_system_call);

    if is_evm_interpreter {
        assert_eq!(stipend, 0);
        stipend = EVM_SIMULATOR_STIPEND;
    }

    let new_frame_gas = new_frame_gas
        .checked_add(stipend)
        .expect("stipend must not cause overflow");

    vm.push_frame::<CALLING_MODE>(
        instruction,
        u256_into_address(destination_address),
        program,
        new_frame_gas,
        stipend,
        exception_handler,
        IS_STATIC && !is_evm_interpreter,
        calldata.memory_page,
        vm.world_diff.snapshot(),
    );

    vm.state.flags = Flags::new(false, false, false);

    if abi.is_system_call {
        // r3 to r12 are kept but they lose their pointer flags
        vm.state.registers[13] = U256::zero();
        vm.state.registers[14] = U256::zero();
        vm.state.registers[15] = U256::zero();
    } else {
        vm.state.registers = [U256::zero(); 16];
    }

    // Only r1 is a pointer
    vm.state.register_pointer_flags = 2;
    vm.state.registers[1] = calldata.into_u256();

    let is_static_call_to_evm_interpreter = IS_STATIC && is_evm_interpreter;
    let call_type = (u8::from(is_static_call_to_evm_interpreter) << 2)
        | (u8::from(abi.is_system_call) << 1)
        | u8::from(abi.is_constructor_call);

    vm.state.registers[2] = call_type.into();

    Ok(vm.state.current_frame.program.instruction(0).unwrap())
}

pub(crate) struct FarCallABI {
    pub memory_quasi_fat_pointer: FatPointer,
    pub gas_to_pass: u32,
    pub shard_id: u8,
    pub forwarding_mode: FarCallForwardPageType,
    pub is_constructor_call: bool,
    pub is_system_call: bool,
}

pub(crate) fn get_far_call_arguments(abi: U256) -> FarCallABI {
    let memory_quasi_fat_pointer = FatPointer::from(abi);
    let gas_to_pass = abi.0[3] as u32;
    let settings = (abi.0[3] >> 32) as u32;
    let [forwarding_byte, shard_id, constructor_call_byte, system_call_byte] =
        settings.to_le_bytes();

    let forwarding_mode = FarCallForwardPageType::from_u8(forwarding_byte);

    FarCallABI {
        memory_quasi_fat_pointer,
        gas_to_pass,
        shard_id,
        forwarding_mode,
        is_constructor_call: constructor_call_byte != 0,
        is_system_call: system_call_byte != 0,
    }
}

pub(crate) fn get_far_call_calldata(
    raw_abi: U256,
    is_pointer: bool,
    vm: &mut VirtualMachine,
) -> Option<FatPointer> {
    let mut pointer = FatPointer::from(raw_abi);

    match FatPointerSource::from_abi((raw_abi.0[3] >> 32) as u8) {
        FatPointerSource::ForwardFatPointer => {
            if !is_pointer || pointer.offset > pointer.length {
                return None;
            }

            pointer.narrow();
        }
        FatPointerSource::MakeNewPointer(target) => {
            // This check has to be first so the penalty for an incorrect bound is always paid.
            // It must be paid even in cases where memory growth wouldn't be paid due to other errors.
            let Some(bound) = pointer.start.checked_add(pointer.length) else {
                let _ = vm.state.use_gas(u32::MAX);
                return None;
            };
            if is_pointer || pointer.offset != 0 {
                return None;
            }

            match target {
                ToHeap => {
                    grow_heap::<Heap>(&mut vm.state, bound).ok()?;
                    pointer.memory_page = vm.state.current_frame.heap;
                }
                ToAuxHeap => {
                    grow_heap::<AuxHeap>(&mut vm.state, pointer.start + pointer.length).ok()?;
                    pointer.memory_page = vm.state.current_frame.aux_heap;
                }
            }
        }
    }

    Some(pointer)
}

enum FatPointerSource {
    MakeNewPointer(FatPointerTarget),
    ForwardFatPointer,
}
enum FatPointerTarget {
    ToHeap,
    ToAuxHeap,
}
use FatPointerTarget::*;

impl FatPointerSource {
    pub const fn from_abi(value: u8) -> Self {
        match value {
            0 => Self::MakeNewPointer(ToHeap),
            1 => Self::ForwardFatPointer,
            2 => Self::MakeNewPointer(ToAuxHeap),
            _ => Self::MakeNewPointer(ToHeap), // default
        }
    }
}

impl FatPointer {
    fn narrow(&mut self) {
        self.start += self.offset;
        self.length -= self.offset;
        self.offset = 0;
    }
}

use super::monomorphization::*;

impl Instruction {
    pub fn from_far_call<const MODE: u8>(
        src1: Register1,
        src2: Register2,
        error_handler: Immediate1,
        is_static: bool,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: monomorphize!(far_call [MODE] match_boolean is_static),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_source(&error_handler),
        }
    }
}
