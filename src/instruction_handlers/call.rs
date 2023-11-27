use super::{heap_access::grow_heap, ret_panic, AuxHeap, Heap};
use crate::{
    addressing_modes::{Arguments, Immediate1, Immediate2, Register1, Register2, Source},
    decommit::{decommit, u256_into_address},
    fat_pointer::FatPointer,
    predication::Flags,
    state::{InstructionResult, Panic},
    Instruction, Predicate, State,
};
use u256::U256;

#[repr(u8)]
pub enum CallingMode {
    Normal,
    Delegate,
    Mimic,
}

fn far_call<const CALLING_MODE: u8, const IS_STATIC: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let address_mask: U256 = U256::MAX >> (256 - 160);

    let abi = match get_far_call_arguments(args, state) {
        Ok(abi) => abi,
        Err(panic) => return ret_panic(state, panic),
    };
    let destination_address = Register2::get(args, state) & address_mask;
    let error_handler = Immediate1::get(args, state);

    let (program, code_page, code_info) = match decommit(&mut state.world, destination_address) {
        Ok(x) => x,
        Err(panic) => return ret_panic(state, panic),
    };
    if !code_info.is_constructed && !abi.is_constructor_call {
        return ret_panic(state, Panic::CallingCodeThatIsNotYetConstructed);
    }

    let maximum_gas = (state.current_frame.gas as u64 * 63 / 64) as u32;
    let new_frame_gas = if abi.gas_to_pass == 0 {
        maximum_gas
    } else {
        abi.gas_to_pass.min(maximum_gas)
    };

    state.current_frame.gas -= new_frame_gas;
    state.push_frame::<CALLING_MODE>(
        instruction,
        u256_into_address(destination_address),
        program,
        code_page,
        new_frame_gas,
        error_handler.low_u32(),
    );

    state.flags = Flags::new(false, false, false);

    if abi.is_system_call {
        state.registers[14] = U256::zero();
        state.registers[15] = U256::zero();
        state.registers[2] = 2.into();
    } else if abi.is_constructor_call {
        // TODO not sure what exactly should be done in this case
        state.registers = [U256::zero(); 16];
        state.registers[2] = 1.into();
    } else {
        state.registers = [U256::zero(); 16];
    }
    state.registers[1] = abi.pointer.into_u256();
    state.register_pointer_flags = 2;

    Ok(&state.current_frame.program[0])
}

pub(crate) struct FarCallABI {
    pub pointer: FatPointer,
    pub gas_to_pass: u32,
    pub shard_id: u8,
    pub is_constructor_call: bool,
    pub is_system_call: bool,
}

pub(crate) fn get_far_call_arguments(
    args: &Arguments,
    state: &mut State,
) -> Result<FarCallABI, Panic> {
    let abi = Register1::get(args, state);
    let gas_to_pass = abi.0[3] as u32;
    let settings = (abi.0[3] >> 32) as u32;
    let [pointer_source, shard_id, constructor_call_byte, system_call_byte] =
        settings.to_le_bytes();

    let mut pointer = FatPointer::from(abi);

    match FatPointerSource::from_abi(pointer_source) {
        FatPointerSource::ForwardFatPointer => {
            if !Register1::is_fat_pointer(args, state) {
                return Err(Panic::IncorrectPointerTags);
            }

            if pointer.offset > pointer.length {
                return Err(Panic::PointerOffsetTooLarge);
            }

            pointer.narrow();
        }
        FatPointerSource::MakeNewPointer(target) => {
            let Some(bound) = pointer.start.checked_add(pointer.length) else {
                return Err(Panic::PointerUpperBoundOverflows);
            };
            if pointer.offset != 0 {
                return Err(Panic::PointerOffsetNotZeroAtCreation);
            }

            match target {
                ToHeap => {
                    grow_heap::<Heap>(state, bound)?;
                    pointer.memory_page = state.current_frame.heap;
                }
                ToAuxHeap => {
                    grow_heap::<AuxHeap>(state, pointer.start + pointer.length)?;
                    pointer.memory_page = state.current_frame.aux_heap;
                }
            }
        }
    }

    Ok(FarCallABI {
        pointer,
        gas_to_pass,
        shard_id,
        is_constructor_call: constructor_call_byte != 0,
        is_system_call: system_call_byte != 0,
    })
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

fn near_call(state: &mut State, mut instruction: *const Instruction) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_to_pass = Register1::get(args, state).0[0] as u32;
    let destination = Immediate1::get(args, state);
    let error_handler = Immediate2::get(args, state);

    let new_frame_gas = if gas_to_pass == 0 {
        state.current_frame.gas
    } else {
        gas_to_pass.min(state.current_frame.gas)
    };
    state
        .current_frame
        .push_near_call(new_frame_gas, instruction, error_handler.low_u32());

    state.flags = Flags::new(false, false, false);

    Ok(&state.current_frame.program[destination.low_u32() as usize])
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
