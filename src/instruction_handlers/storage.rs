use zkevm_opcode_defs::Opcode;

use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        Arguments, Destination, Register1, Register2, Source, SLOAD_COST, SSTORE_COST,
    },
    instruction::InstructionResult,
    vm::{STORAGE_READ_STORAGE_APPLICATION_CYCLES, STORAGE_WRITE_STORAGE_APPLICATION_CYCLES},
    Instruction, VirtualMachine, World,
};

fn sstore(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, world| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);
        let contract = vm.state.current_frame.address;

        if !vm
            .world_diff
            .written_storage_slots_ct
            .contains(&(contract, key))
        {
            vm.statistics.storage_application_cycles += STORAGE_WRITE_STORAGE_APPLICATION_CYCLES;
        }
        let refund = vm.world_diff.write_storage(world, contract, key, value);

        assert!(refund <= SSTORE_COST);
        vm.state.current_frame.gas += refund;
    })
}

fn sstore_transient(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        vm.world_diff
            .write_transient_storage(vm.state.current_frame.address, key, value);
    })
}

fn sload(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, world| {
        let key = Register1::get(args, &mut vm.state);
        let address = vm.state.current_frame.address;

        if !vm
            .world_diff
            .read_storage_slots_ct
            .contains(&(address, key))
            && !vm
                .world_diff
                .written_storage_slots_ct
                .contains(&(address, key))
        {
            vm.statistics.storage_application_cycles += STORAGE_READ_STORAGE_APPLICATION_CYCLES;
        }
        let (value, refund) = vm.world_diff.read_storage(world, address, key);
        assert!(refund <= SLOAD_COST);
        vm.state.current_frame.gas += refund;

        Register1::set(args, &mut vm.state, value);
    })
}

fn sload_transient(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate(vm, instruction, world, |vm, args, _| {
        let key = Register1::get(args, &mut vm.state);
        let value = vm
            .world_diff
            .read_transient_storage(vm.state.current_frame.address, key);

        Register1::set(args, &mut vm.state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore(
        opcode: Opcode,
        src1: Register1,
        src2: Register2,
        arguments: Arguments,
    ) -> Self {
        Self {
            opcode,
            handler: sstore,
            arguments: arguments.write_source(&src1).write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore_transient(
        opcode: Opcode,
        src1: Register1,
        src2: Register2,
        arguments: Arguments,
    ) -> Self {
        Self {
            opcode,
            handler: sstore_transient,
            arguments: arguments.write_source(&src1).write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload(
        opcode: Opcode,
        src: Register1,
        dst: Register1,
        arguments: Arguments,
    ) -> Self {
        Self {
            opcode,
            handler: sload,
            arguments: arguments.write_source(&src).write_destination(&dst),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload_transient(
        opcode: Opcode,
        src: Register1,
        dst: Register1,
        arguments: Arguments,
    ) -> Self {
        Self {
            opcode,
            handler: sload_transient,
            arguments: arguments.write_source(&src).write_destination(&dst),
        }
    }
}
