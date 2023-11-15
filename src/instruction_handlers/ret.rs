use super::call::get_far_call_arguments;
use crate::{
    addressing_modes::{Arguments, Register1},
    predication::Flags,
    state::{ExecutionEnd, InstructionResult},
    Instruction, Predicate, State,
};
use u256::U256;

fn ret(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_left = state.current_frame.gas;

    let pc = if let Some(pc) = state.current_frame.pop_near_call() {
        pc
    } else {
        let return_value = get_far_call_arguments(args, state)?.pointer;

        // TODO check that the return value resides in this or a newer frame's memory

        let Some(pc) = state.pop_frame() else {
            return Err(ExecutionEnd::ProgramFinished(state.heaps[return_value.memory_page as usize][return_value.start as usize..(return_value.start + return_value.length) as usize].to_vec()));
        };

        state.set_context_u128(0);

        state.registers = [U256::zero(); 16];
        state.registers[1] = return_value.into_u256();
        state.register_pointer_flags = 2;
        pc
    };

    state.flags = Flags::new(false, false, false);
    state.current_frame.gas += gas_left;

    Ok(unsafe { pc.add(1) })
}

impl Instruction {
    pub fn from_ret(src1: Register1, predicate: Predicate) -> Self {
        Self {
            handler: ret,
            arguments: Arguments::new(predicate, 5).write_source(&src1),
        }
    }
}
