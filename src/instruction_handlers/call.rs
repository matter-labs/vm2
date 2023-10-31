use super::{pointer::FatPointer, ret};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source, SourceWriter},
    predication::Flags,
    Instruction, Predicate, State,
};
use u256::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
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

fn far_call<const IS_STATIC: bool>(state: &mut State, instruction: *const Instruction) {
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

    let deployer_system_contract_address =
        Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);
    let code_info = state
        .world
        .read_storage(deployer_system_contract_address, destination_address);

    // TODO default address aliasing

    let mut code_info_bytes = [0; 32];
    code_info.to_big_endian(&mut code_info_bytes);

    if code_info_bytes[0] != 1 {
        return ret::panic();
    }
    match code_info_bytes[1] {
        0 => {} // At rest
        1 => {} // constructed
        _ => {
            return ret::panic();
        }
    }
    let code_length_in_words = u16::from_be_bytes([code_info_bytes[2], code_info_bytes[3]]);

    code_info_bytes[1] = 0;
    let code_key: U256 = U256::from_big_endian(&code_info_bytes);

    // TODO pay based on program length

    let (program, code_page) = state.world.decommit(code_key);

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
    state.registers[0] = pointer_to_arguments.to_u256();
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
