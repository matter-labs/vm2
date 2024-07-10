use super::{
    heap_access::grow_heap,
    ret::{panic_from_failed_far_call, RETURN_COST},
    AuxHeap, Heap,
};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    decommit::{is_kernel, u256_into_address},
    fat_pointer::FatPointer,
    instruction::{Handler, InstructionResult},
    predication::Flags,
    Instruction, VirtualMachine, World,
};
use u256::U256;
use zkevm_opcode_defs::{
    system_params::{EVM_SIMULATOR_STIPEND, MSG_VALUE_SIMULATOR_ADDITIVE_COST},
    ADDRESS_MSG_VALUE,
};

#[repr(u8)]
pub enum CallingMode {
    Normal,
    Delegate,
    Mimic,
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
fn far_call<const CALLING_MODE: u8, const IS_STATIC: bool, const IS_SHARD: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let (raw_abi, raw_abi_is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);

    let address_mask: U256 = U256::MAX >> (256 - 160);
    let destination_address = Register2::get(args, &mut vm.state) & address_mask;
    let exception_handler = Immediate1::get(args, &mut vm.state).low_u32() as u16;

    let mut abi = get_far_call_arguments(raw_abi);
    abi.is_constructor_call = abi.is_constructor_call && vm.state.current_frame.is_kernel;
    abi.is_system_call = abi.is_system_call && is_kernel(u256_into_address(destination_address));

    let mut mandated_gas = if abi.is_system_call && destination_address == ADDRESS_MSG_VALUE.into()
    {
        MSG_VALUE_SIMULATOR_ADDITIVE_COST
    } else {
        0
    };

    let failing_part = (|| {
        let decommit_result = vm.world_diff.decommit(
            world,
            destination_address,
            vm.settings.default_aa_code_hash,
            vm.settings.evm_interpreter_code_hash,
            abi.is_constructor_call,
        );

        // calldata has to be constructed even if we already know we will panic because
        // overflowing start + length makes the heap resize even when already panicking.
        let already_failed = decommit_result.is_none() || IS_SHARD && abi.shard_id != 0;

        let maybe_calldata = get_far_call_calldata(raw_abi, raw_abi_is_pointer, vm, already_failed);

        // mandated gas is passed even if it means transferring more than the 63/64 rule allows
        if let Some(gas_left) = vm.state.current_frame.gas.checked_sub(mandated_gas) {
            vm.state.current_frame.gas = gas_left;
        } else {
            // If the gas is insufficient, the rest is burned
            vm.state.current_frame.gas = 0;
            mandated_gas = 0;
            return None;
        };

        let calldata = maybe_calldata?;
        let (unpaid_decommit, is_evm) = decommit_result?;
        let program = vm.world_diff.pay_for_decommit(
            world,
            unpaid_decommit,
            &mut vm.state.current_frame.gas,
        )?;

        Some((calldata, program, is_evm))
    })();

    let maximum_gas = vm.state.current_frame.gas / 64 * 63;
    let normally_passed_gas = abi.gas_to_pass.min(maximum_gas);
    vm.state.current_frame.gas -= normally_passed_gas;
    let new_frame_gas = normally_passed_gas + mandated_gas;

    let Some((calldata, program, is_evm_interpreter)) = failing_part else {
        vm.state.current_frame.gas += new_frame_gas.saturating_sub(RETURN_COST);
        return panic_from_failed_far_call(vm, exception_handler);
    };

    let stipend = if is_evm_interpreter {
        EVM_SIMULATOR_STIPEND
    } else {
        0
    };
    let new_frame_gas = new_frame_gas
        .checked_add(stipend)
        .expect("stipend must not cause overflow");

    let new_frame_is_static = IS_STATIC || vm.state.current_frame.is_static;
    vm.push_frame::<CALLING_MODE>(
        instruction,
        u256_into_address(destination_address),
        program,
        new_frame_gas,
        stipend,
        exception_handler,
        new_frame_is_static && !is_evm_interpreter,
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

    let is_static_call_to_evm_interpreter = new_frame_is_static && is_evm_interpreter;
    let call_type = (u8::from(is_static_call_to_evm_interpreter) << 2)
        | (u8::from(abi.is_system_call) << 1)
        | u8::from(abi.is_constructor_call);

    vm.state.registers[2] = call_type.into();

    Ok(vm.state.current_frame.program.instruction(0).unwrap())
}

pub(crate) struct FarCallABI {
    pub gas_to_pass: u32,
    pub shard_id: u8,
    pub is_constructor_call: bool,
    pub is_system_call: bool,
}

pub(crate) fn get_far_call_arguments(abi: U256) -> FarCallABI {
    let gas_to_pass = abi.0[3] as u32;
    let settings = (abi.0[3] >> 32) as u32;
    let [_, shard_id, constructor_call_byte, system_call_byte] = settings.to_le_bytes();

    FarCallABI {
        gas_to_pass,
        shard_id,
        is_constructor_call: constructor_call_byte != 0,
        is_system_call: system_call_byte != 0,
    }
}

/// Forms a new fat pointer or narrows an existing one, as dictated by the ABI.
///
/// This function needs to be called even if we already know we will panic because
/// overflowing start + length makes the heap resize even when already panicking.
pub(crate) fn get_far_call_calldata(
    raw_abi: U256,
    is_pointer: bool,
    vm: &mut VirtualMachine,
    already_failed: bool,
) -> Option<FatPointer> {
    let mut pointer = FatPointer::from(raw_abi);

    match FatPointerSource::from_abi((raw_abi.0[3] >> 32) as u8) {
        FatPointerSource::ForwardFatPointer => {
            if !is_pointer || pointer.offset > pointer.length || already_failed {
                return None;
            }

            pointer.narrow();
        }
        FatPointerSource::MakeNewPointer(target) => {
            if let Some(bound) = pointer.start.checked_add(pointer.length) {
                if is_pointer || pointer.offset != 0 || already_failed {
                    return None;
                }
                match target {
                    FatPointerTarget::ToHeap => {
                        grow_heap::<Heap>(&mut vm.state, bound).ok()?;
                        pointer.memory_page = vm.state.current_frame.heap;
                    }
                    FatPointerTarget::ToAuxHeap => {
                        grow_heap::<AuxHeap>(&mut vm.state, bound).ok()?;
                        pointer.memory_page = vm.state.current_frame.aux_heap;
                    }
                }
            } else {
                // The heap is grown even if the pointer goes out of the heap
                // TODO PLA-974 revert to not growing the heap on failure as soon as zk_evm is fixed
                let bound = u32::MAX;
                match target {
                    FatPointerTarget::ToHeap => {
                        grow_heap::<Heap>(&mut vm.state, bound).ok()?;
                    }
                    FatPointerTarget::ToAuxHeap => {
                        grow_heap::<AuxHeap>(&mut vm.state, bound).ok()?;
                    }
                }
                return None;
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

impl FatPointerSource {
    pub const fn from_abi(value: u8) -> Self {
        match value {
            0 => Self::MakeNewPointer(FatPointerTarget::ToHeap),
            1 => Self::ForwardFatPointer,
            2 => Self::MakeNewPointer(FatPointerTarget::ToAuxHeap),
            _ => Self::MakeNewPointer(FatPointerTarget::ToHeap), // default
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
        is_shard: bool,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: Handler::Jump(
                monomorphize!(far_call [MODE] match_boolean is_static match_boolean is_shard),
            ),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_source(&error_handler),
        }
    }
}
