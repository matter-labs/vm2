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
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);
        let address = vm.state.current_frame.address;

        let update_cost = vm.world.world.cost_of_writing_storage(address, key, value);
        let prepaid = vm.world.prepaid_for_write(address, key);

        // Note, that the diff may be negative, e.g. in case the new write returns to the previous value.
        vm.state.current_frame.total_pubdata_spent += (update_cost as i32) - (prepaid as i32);

        vm.world.insert_prepaid_for_write(address, key, update_cost);

        if vm.state.current_frame.is_static {
            return Ok(&PANIC);
        }

        let refund = vm.world.write_storage(address, key, value);

        assert!(refund <= SSTORE_COST);
        vm.state.current_frame.gas += refund;

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
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        Self {
            handler: sload,
            arguments: Arguments::new(predicate, SLOAD_COST)
                .write_source(&src)
                .write_destination(&dst),
        }
    }
}
