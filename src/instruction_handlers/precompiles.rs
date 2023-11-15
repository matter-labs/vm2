use crate::{
    addressing_modes::{Arguments, Register1, Register2, Source},
    keccak,
    state::InstructionResult,
    Instruction, Predicate, State,
};
use zkevm_opcode_defs::{
    system_params::{
        ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS, KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
        SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
    },
    PrecompileCallABI,
};

use super::common::instruction_boilerplate_with_panic;

fn precompile_call(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
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
            SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                todo!()
            }
            ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS => {
                todo!()
            }
            _ => {
                // A precompile call may be used just to burn gas
            }
        }

        Ok(())
    })
}

impl Instruction {
    pub fn from_precompile_call(abi: Register1, burn: Register2, predicate: Predicate) -> Self {
        Self {
            arguments: Arguments::new(predicate, 6)
                .write_source(&abi)
                .write_source(&burn),
            handler: precompile_call,
        }
    }
}
