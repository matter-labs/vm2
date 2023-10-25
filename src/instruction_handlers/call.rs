use super::{pointer::FatPointer, ret};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source, SourceWriter},
    Instruction, Predicate, State, World,
};

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

fn far_call<W: World, const IS_STATIC: bool>(
    state: &mut State<W>,
    instruction: *const Instruction<W>,
) {
    let args = unsafe { &(*instruction).arguments };

    let settings_and_pointer = Register1::get(args, state);
    let destination_address = Register2::get(args, state);
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
                    return ret::panic();
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

    let (program, code_page) = state.world.decommit();

    let maximum_gas = (state.current_frame.gas as u64 * 63 / 64) as u32;
    let new_frame_gas = if gas_to_pass == 0 {
        maximum_gas
    } else {
        gas_to_pass.min(maximum_gas)
    };

    state.current_frame.gas -= new_frame_gas;
    state.push_frame(instruction, program, code_page, new_frame_gas)
}

impl FatPointer {
    fn narrow(&mut self) {
        self.start += self.offset;
        self.length -= self.offset;
        self.offset = 0;
    }
}

use super::monomorphization::*;

impl<W: World> Instruction<W> {
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
            handler: monomorphize!(far_call [W] match_boolean is_static),
            arguments: args,
        }
    }
}
