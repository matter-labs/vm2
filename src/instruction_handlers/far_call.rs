use super::{heap_access::grow_heap, ret::INVALID_INSTRUCTION, AuxHeap, Heap};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    decommit::u256_into_address,
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    predication::Flags,
    rollback::Rollback,
    Instruction, Predicate, VirtualMachine,
};
use u256::U256;
use zkevm_opcode_defs::system_params::EVM_SIMULATOR_STIPEND;

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
fn far_call<const CALLING_MODE: u8, const IS_STATIC: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let address_mask: U256 = U256::MAX >> (256 - 160);

    let raw_abi = Register1::get(args, &mut vm.state);
    let destination_address = Register2::get(args, &mut vm.state) & address_mask;
    let exception_handler = Immediate1::get(args, &mut vm.state).low_u32() as u16;

    let abi = get_far_call_arguments(raw_abi);

    let calldata =
        get_far_call_calldata(raw_abi, Register1::is_fat_pointer(args, &mut vm.state), vm);

    let decommit_result = vm.world.decommit(
        destination_address,
        vm.settings.default_aa_code_hash,
        vm.settings.evm_interpreter_code_hash,
        &mut vm.state.current_frame.gas,
        abi.is_constructor_call,
    );

    let maximum_gas = (vm.state.current_frame.gas / 64 * 63) as u32;
    let new_frame_gas = abi.gas_to_pass.min(maximum_gas);
    vm.state.current_frame.gas -= new_frame_gas;

    let (Some(calldata), Some((program, is_evm_interpreter))) = (calldata, decommit_result) else {
        vm.state
            .push_dummy_frame(instruction, exception_handler, vm.world.snapshot());
        return Ok(&INVALID_INSTRUCTION);
    };

    let stipend = if is_evm_interpreter {
        EVM_SIMULATOR_STIPEND
    } else {
        0
    };
    let new_frame_gas = new_frame_gas
        .checked_add(stipend)
        .expect("stipend must not cause overflow");

    vm.state.push_frame::<CALLING_MODE>(
        instruction,
        u256_into_address(destination_address),
        program,
        new_frame_gas,
        stipend,
        exception_handler,
        IS_STATIC && !is_evm_interpreter,
        calldata.memory_page,
        vm.world.snapshot(),
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

    Ok(&vm.state.current_frame.program.instructions()[0])
}

pub(crate) struct FarCallABI {
    pub gas_to_pass: u32,
    pub _shard_id: u8,
    pub is_constructor_call: bool,
    pub is_system_call: bool,
}

pub(crate) fn get_far_call_arguments(abi: U256) -> FarCallABI {
    let gas_to_pass = abi.0[3] as u32;
    let settings = (abi.0[3] >> 32) as u32;
    let [_, _shard_id, constructor_call_byte, system_call_byte] = settings.to_le_bytes();

    FarCallABI {
        gas_to_pass,
        _shard_id,
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
        predicate: Predicate,
    ) -> Self {
        Self {
            handler: monomorphize!(far_call [MODE] match_boolean is_static),
            arguments: Arguments::new(predicate, 182)
                .write_source(&src1)
                .write_source(&src2)
                .write_source(&error_handler),
        }
    }
}
