use std::sync::Arc;

use super::{heap_access::grow_heap, ret_panic, AuxHeap, Heap};
use crate::{
    addressing_modes::{Arguments, Immediate1, Immediate2, Register1, Register2, Source},
    decommit::u256_into_address,
    fat_pointer::FatPointer,
    instruction::{InstructionResult, Panic},
    predication::Flags,
    rollback::Rollback,
    Instruction, Predicate, VirtualMachine,
};
use u256::U256;

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
    let error_handler = Immediate1::get(args, &mut vm.state);

    let abi = get_far_call_arguments(raw_abi);

    let mut encountered_panic = None;

    let calldata =
        match get_far_call_calldata(raw_abi, Register1::is_fat_pointer(args, &mut vm.state), vm) {
            Ok(pointer) => pointer.into_u256(),
            Err(panic) => {
                encountered_panic = Some(panic);
                U256::zero()
            }
        };

    let (program, code_page) = match vm.world.decommit(
        destination_address,
        vm.settings.default_aa_code_hash,
        &mut vm.state.current_frame.gas,
        abi.is_constructor_call,
    ) {
        Ok(program) => program,
        Err(panic) => {
            encountered_panic = Some(panic);
            let substitute: (Arc<[Instruction]>, Arc<[U256]>) = (Arc::new([]), Arc::new([]));
            substitute
        }
    };

    let maximum_gas = (vm.state.current_frame.gas / 64 * 63) as u32;
    let new_frame_gas = abi.gas_to_pass.min(maximum_gas);

    vm.state.current_frame.gas -= new_frame_gas;
    vm.state.push_frame::<CALLING_MODE>(
        instruction,
        u256_into_address(destination_address),
        program,
        code_page,
        new_frame_gas,
        error_handler.low_u32(),
        IS_STATIC,
        vm.world.snapshot(),
    );

    if let Some(panic) = encountered_panic {
        return ret_panic(vm, panic);
    }

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
    vm.state.registers[1] = calldata;

    let is_static_call_to_evm_interpreter = IS_STATIC && false;
    let call_type = (u8::from(is_static_call_to_evm_interpreter) << 2)
        | (u8::from(abi.is_system_call) << 1)
        | u8::from(abi.is_constructor_call);

    vm.state.registers[2] = call_type.into();

    Ok(&vm.state.current_frame.program[0])
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
) -> Result<FatPointer, Panic> {
    let mut pointer = FatPointer::from(raw_abi);

    match FatPointerSource::from_abi((raw_abi.0[3] >> 32) as u8) {
        FatPointerSource::ForwardFatPointer => {
            if !is_pointer {
                return Err(Panic::IncorrectPointerTags);
            }

            if pointer.offset > pointer.length {
                return Err(Panic::PointerOffsetTooLarge);
            }

            pointer.narrow();
        }
        FatPointerSource::MakeNewPointer(target) => {
            // This check has to be first so the penalty for an incorrect bound is always paid.
            // It must be paid even in cases where memory growth wouldn't be paid due to other errors.
            let Some(bound) = pointer.start.checked_add(pointer.length) else {
                let _ = vm.state.use_gas(u32::MAX);
                return Err(Panic::PointerUpperBoundOverflows);
            };
            if is_pointer {
                return Err(Panic::IncorrectPointerTags);
            }
            if pointer.offset != 0 {
                return Err(Panic::PointerOffsetNotZeroAtCreation);
            }

            match target {
                ToHeap => {
                    grow_heap::<Heap>(&mut vm.state, bound)?;
                    pointer.memory_page = vm.state.current_frame.heap;
                }
                ToAuxHeap => {
                    grow_heap::<AuxHeap>(&mut vm.state, pointer.start + pointer.length)?;
                    pointer.memory_page = vm.state.current_frame.aux_heap;
                }
            }
        }
    }

    Ok(pointer)
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

fn near_call(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_to_pass = Register1::get(args, &mut vm.state).0[0] as u32;
    let destination = Immediate1::get(args, &mut vm.state);
    let error_handler = Immediate2::get(args, &mut vm.state);

    let new_frame_gas = if gas_to_pass == 0 {
        vm.state.current_frame.gas
    } else {
        gas_to_pass.min(vm.state.current_frame.gas)
    };
    vm.state.current_frame.push_near_call(
        new_frame_gas,
        instruction,
        error_handler.low_u32(),
        vm.world.snapshot(),
    );

    vm.state.flags = Flags::new(false, false, false);

    Ok(&vm.state.current_frame.program[destination.low_u32() as usize])
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

impl Instruction {
    pub fn from_near_call(
        gas: Register1,
        destination: Immediate1,
        error_handler: Immediate2,
        predicate: Predicate,
    ) -> Self {
        Self {
            handler: near_call,
            arguments: Arguments::new(predicate, 25)
                .write_source(&gas)
                .write_source(&destination)
                .write_source(&error_handler),
        }
    }
}
