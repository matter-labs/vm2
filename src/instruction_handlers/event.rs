use super::{common::instruction_boilerplate_with_panic, free_panic};
use crate::{
    addressing_modes::{Arguments, Immediate1, Register1, Register2, Source},
    instruction::InstructionResult,
    world_diff::{Event, L2ToL1Log},
    Instruction, VirtualMachine, World,
};
use u256::H160;
use zkevm_opcode_defs::ADDRESS_EVENT_WRITER;

fn event(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate_with_panic(
        vm,
        instruction,
        world,
        |vm, args, world, continue_normally| {
            if vm.state.current_frame.is_static {
                return free_panic(vm, world);
            }
            if vm.state.current_frame.address == H160::from_low_u64_be(ADDRESS_EVENT_WRITER as u64)
            {
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

            continue_normally
        },
    )
}

fn l2_to_l1(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate_with_panic(
        vm,
        instruction,
        world,
        |vm, args, world, continue_normally| {
            if vm.state.current_frame.is_static {
                return free_panic(vm, world);
            }

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

            continue_normally
        },
    )
}

impl Instruction {
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
