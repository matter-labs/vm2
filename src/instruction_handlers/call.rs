use super::pointer::FatPointer;
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source, SourceWriter},
    decommit::decommit,
    predication::Flags,
    state::{ExecutionResult, Panic},
    Instruction, Predicate, State,
};
use u256::{H160, U256};

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

fn far_call<const IS_STATIC: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    let address_mask: U256 = U256::MAX >> (256 - 160);

    let settings_and_pointer = Register1::get(args, state);
    let destination_address = Register2::get(args, state) & address_mask;
    let error_handler = Immediate1::get(args, state);

    let settings = settings_and_pointer.0[3];
    let gas_to_pass = settings as u32;
    let [pointer_source, shard_id, constructor_call_byte, system_call_byte] =
        ((settings >> 32) as u32).to_le_bytes();

    let is_constructor_call = constructor_call_byte != 0;
    let is_system_call = system_call_byte != 0;

    let pointer_to_arguments = {
        let mut out = FatPointer::from(settings_and_pointer);

        match FatPointerSource::from_abi(pointer_source) {
            FatPointerSource::ForwardFatPointer => {
                if !Register1::is_fat_pointer(args, state) {
                    return Err(Panic::IncorrectPointerTags);
                }

                // TODO check validity

                out.narrow();
            }
            FatPointerSource::MakeNewPointerToHeap => {
                out.memory_page = state.current_frame.heap;
                // TODO grow heap
            }
            FatPointerSource::MakeNewPointerToAuxHeap => {
                out.memory_page = state.current_frame.aux_heap;
                // TODO grow heap
            }
        }

        out
    };

    let (program, code_page) = decommit(&mut state.world, destination_address);

    let maximum_gas = (state.current_frame.gas as u64 * 63 / 64) as u32;
    let new_frame_gas = if gas_to_pass == 0 {
        maximum_gas
    } else {
        gas_to_pass.min(maximum_gas)
    };

    state.current_frame.gas -= new_frame_gas;
    state.push_frame(instruction, H160::zero(), program, code_page, new_frame_gas);

    // TODO clear context register

    state.flags = Flags::new(false, false, false);

    state.registers = [U256::zero(); 16];
    state.registers[0] = pointer_to_arguments.into_u256();
    state.register_pointer_flags = 1;

    state.run()
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
