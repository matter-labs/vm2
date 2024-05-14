use super::{
    common::{instruction_boilerplate, instruction_boilerplate_with_panic},
    PANIC,
};
use crate::{
    addressing_modes::{
        Arguments, Destination, Register1, Register2, Source, SLOAD_COST, SSTORE_COST,
    },
    instruction::InstructionResult,
    Instruction, Predicate, VirtualMachine,
};

fn sstore(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        if vm.state.current_frame.is_static {
            return Ok(&PANIC);
        }

        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        let (refund, pubdata_change) =
            vm.world
                .write_storage(vm.state.current_frame.address, key, value);

        assert!(refund <= SSTORE_COST);
        vm.state.current_frame.gas += refund;

        vm.state.current_frame.total_pubdata_spent += pubdata_change;

        continue_normally
    })
}

fn sstore_transient(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        if vm.state.current_frame.is_static {
            return Ok(&PANIC);
        }

        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        vm.world
            .write_transient_storage(vm.state.current_frame.address, key, value);

        continue_normally
    })
}

fn sload(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(vm, instruction, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let (value, refund) = vm.world.read_storage(vm.state.current_frame.address, key);

        assert!(refund <= SLOAD_COST);
        vm.state.current_frame.gas += refund;

        Register1::set(args, &mut vm.state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        Self {
            handler: sstore,
            arguments: Arguments::new(predicate, SSTORE_COST)
                .write_source(&src1)
                .write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore_transient(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        Self {
            handler: sstore_transient,
            arguments: Arguments::new(predicate, 0)
                .write_source(&src1)
                .write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        Self {
            handler: sload,
            arguments: Arguments::new(predicate, SLOAD_COST)
                .write_source(&src)
                .write_destination(&dst),
        }
    }
}

fn sload_transient(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(vm, instruction, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let value = vm
            .world
            .read_transient_storage(vm.state.current_frame.address, key);

        Register1::set(args, &mut vm.state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload_transient(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        Self {
            handler: sload_transient,
            arguments: Arguments::new(predicate, 0)
                .write_source(&src)
                .write_destination(&dst),
        }
    }
}
