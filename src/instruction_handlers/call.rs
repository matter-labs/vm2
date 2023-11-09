use crate::{
    addressing_modes::{
        Arguments, Immediate1, Immediate2, Register1, Register2, Source, SourceWriter,
    },
    decommit::{decommit, u256_into_address},
    fat_pointer::FatPointer,
    predication::Flags,
    state::{ExecutionResult, Panic},
    Instruction, Predicate, State,
};
use u256::U256;

fn far_call<const IS_STATIC: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    let address_mask: U256 = U256::MAX >> (256 - 160);

    let abi = get_far_call_arguments(args, state)?;
    let destination_address = Register2::get(args, state) & address_mask;
    let error_handler = Immediate1::get(args, state);

    let (program, code_page) = decommit(&mut state.world, destination_address);

    let maximum_gas = (state.current_frame.gas as u64 * 63 / 64) as u32;
    let new_frame_gas = if abi.gas_to_pass == 0 {
        maximum_gas
    } else {
        abi.gas_to_pass.min(maximum_gas)
    };

    state.current_frame.gas -= new_frame_gas;
    state.push_frame(
        instruction,
        u256_into_address(destination_address),
        program,
        code_page,
        new_frame_gas,
    );

    // TODO clear context register

    state.flags = Flags::new(false, false, false);

    state.registers = [U256::zero(); 16];
    state.registers[1] = abi.pointer.into_u256();
    state.register_pointer_flags = 2;

    state.run()
}

pub(crate) struct FarCallABI {
    pub pointer: FatPointer,
    pub gas_to_pass: u32,
    pub shard_id: u8,
    pub constructor_call: bool,
    pub system_call: bool,
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

            // TODO check validity

            pointer.narrow();
        }
        FatPointerSource::MakeNewPointerToHeap => {
            grow_heap::<Heap>(state, pointer.start + pointer.length)?;
            pointer.memory_page = state.current_frame.heap;
        }
        FatPointerSource::MakeNewPointerToAuxHeap => {
            grow_heap::<AuxHeap>(state, pointer.start + pointer.length)?;
            pointer.memory_page = state.current_frame.aux_heap;
        }
    }

    Ok(FarCallABI {
        pointer,
        gas_to_pass,
        shard_id,
        constructor_call: constructor_call_byte != 0,
        system_call: system_call_byte != 0,
    })
}

enum FatPointerSource {
    MakeNewPointerToHeap,
    ForwardFatPointer,
    MakeNewPointerToAuxHeap,
}

impl FatPointerSource {
    pub const fn from_abi(value: u8) -> Self {
        match value {
            0 => Self::MakeNewPointerToHeap,
            1 => Self::ForwardFatPointer,
            2 => Self::MakeNewPointerToAuxHeap,
            _ => Self::MakeNewPointerToHeap, // default
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

fn near_call(state: &mut State, mut instruction: *const Instruction) -> ExecutionResult {
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
        .push_near_call(new_frame_gas, instruction);

    state.flags = Flags::new(false, false, false);

    // Jump!
    unsafe {
        instruction = &state.current_frame.program[destination.low_u32() as usize];
        state.use_gas(1)?;

        while !(*instruction).arguments.predicate.satisfied(&state.flags) {
            instruction = instruction.add(1);
            state.use_gas(1)?;
        }

        ((*instruction).handler)(state, instruction)
    }
}

use super::{heap_access::grow_heap, monomorphization::*, AuxHeap, Heap};

impl Instruction {
    pub fn from_far_call(
        src1: Register1,
        src2: Register2,
        error_handler: Immediate1,
        is_static: bool,
        predicate: Predicate,
    ) -> Self {
        let mut args = Arguments::default();
        src1.write_source(&mut args);
        src2.write_source(&mut args);
        error_handler.write_source(&mut args);
        args.predicate = predicate;

        Self {
            handler: monomorphize!(far_call match_boolean is_static),
            arguments: args,
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
        let mut args = Arguments::default();
        gas.write_source(&mut args);
        destination.write_source(&mut args);
        error_handler.write_source(&mut args);
        args.predicate = predicate;

        Self {
            handler: near_call,
            arguments: args,
        }
    }
}
