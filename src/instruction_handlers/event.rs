use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    instruction::ExecutionStatus,
    world_diff::{Event, L2ToL1Log},
    Instruction, VirtualMachine,
};
use eravm_stable_interface::{opcodes, Tracer};
use u256::H160;
use zkevm_opcode_defs::ADDRESS_EVENT_WRITER;

fn event<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<opcodes::Event, _, _>(vm, world, tracer, |vm, args, _| {
        if vm.state.current_frame.address == H160::from_low_u64_be(ADDRESS_EVENT_WRITER as u64) {
            let key = Register1::get(args, &mut vm.state);
            let value = Register2::get(args, &mut vm.state);
            let is_first = Immediate1::get(args, &mut vm.state).low_u32() == 1;

            vm.world_diff.record_event(Event {
                key,
                value,
                is_first,
                shard_id: 0, // shards currently aren't supported
                tx_number: vm.state.transaction_number,
            });
        }
    })
}

fn l2_to_l1<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<opcodes::L2ToL1Message, _, _>(vm, world, tracer, |vm, args, _| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);
        let is_service = Immediate1::get(args, &mut vm.state).low_u32() == 1;
        vm.world_diff.record_l2_to_l1_log(L2ToL1Log {
            key,
            value,
            is_service,
            address: vm.state.current_frame.address,
            shard_id: 0,
            tx_number: vm.state.transaction_number,
        });
    })
}

impl<T: Tracer, W> Instruction<T, W> {
    pub fn from_event(
        key: Register1,
        value: Register2,
        is_first: bool,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: event,
            arguments: arguments
                .write_source(&key)
                .write_source(&value)
                .write_source(&Immediate1(is_first.into())),
        }
    }

    pub fn from_l2_to_l1_message(
        key: Register1,
        value: Register2,
        is_service: bool,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: l2_to_l1,
            arguments: arguments
                .write_source(&key)
                .write_source(&value)
                .write_source(&Immediate1(is_service.into())),
        }
    }
}
