use crate::{
    addressing_modes::{Arguments, Register1, Register2, Source, SourceWriter},
    keccak,
    state::ExecutionResult,
    Instruction, State,
};
use zkevm_opcode_defs::{
    system_params::KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS, PrecompileCallABI,
};

use super::common::run_next_instruction;

fn precompile_call(state: &mut State, instruction: *const Instruction) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    // TODO check that we're in a system call

    // The user gets to decide how much gas to burn
    // This is safe because system contracts are trusted
    let gas_to_burn = Register2::get(args, state);
    state.use_gas(gas_to_burn.low_u32())?;

    let mut abi = PrecompileCallABI::from_u256(Register1::get(args, state));
    if abi.memory_page_to_read == 0 {
        abi.memory_page_to_read = state.current_frame.heap;
    }
    if abi.memory_page_to_write == 0 {
        abi.memory_page_to_write = state.current_frame.heap;
    }

    let address_bytes = state.current_frame.address.0;
    let address_low = u16::from_le_bytes([address_bytes[19], address_bytes[18]]);
    match address_low {
        KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
            keccak::execute_precompile(abi, &mut state.heaps);
        }
        _ => {
            // A precompile call may be used just to burn gas
        }
    }

    run_next_instruction(state, instruction)
}

impl Instruction {
    pub fn from_precompile_call(abi: Register1, burn: Register2) -> Self {
        let mut arguments = Arguments::default();
        abi.write_source(&mut arguments);
        burn.write_source(&mut arguments);
        Self {
            arguments,
            handler: precompile_call,
        }
    }
}
