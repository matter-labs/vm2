use u256::H160;
use zkevm_opcode_defs::ADDRESS_EVENT_WRITER;

use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    modified_world::Event,
    state::InstructionResult,
    Instruction, Predicate, State,
};

use super::common::instruction_boilerplate_with_panic;

fn event(state: &mut State, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
        if state.current_frame.is_static {
            return Err(crate::state::Panic::WriteInStaticCall);
        }
        if state.current_frame.address == H160::from_low_u64_be(ADDRESS_EVENT_WRITER as u64) {
            let key = Register1::get(args, state);
            let value = Register2::get(args, state);
            let is_first = Immediate1::get(args, state).low_u32() == 1;

            state.world.record_event(Event {
                key,
                value,
                is_first,
                shard_id: 0,  // TODO
                tx_number: 0, // TODO
            });
        }
        Ok(())
    })
}

impl Instruction {
    pub fn from_event(
        key: Register1,
        value: Register2,
        is_first: bool,
        predicate: Predicate,
    ) -> Self {
        Self {
            handler: event,
            arguments: Arguments::new(predicate, 38)
                .write_source(&key)
                .write_source(&value)
                .write_source(&Immediate1(is_first.into())),
        }
    }
}
