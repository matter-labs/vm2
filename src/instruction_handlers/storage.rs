use super::common::{instruction_boilerplate, instruction_boilerplate_with_panic};
use crate::{
    addressing_modes::{
        Arguments, Destination, Register1, Register2, Source, SLOAD_COST, SSTORE_COST,
    },
    instruction::{InstructionResult, Panic},
    Instruction, Predicate, VirtualMachine,
};

fn sstore(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        // TODO: pubdata cost

        if vm.state.current_frame.is_static {
            return Err(Panic::WriteInStaticCall);
        }

        vm.world
            .write_storage(vm.state.current_frame.address, key, value);

        Ok(())
    })
}

fn sload(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(vm, instruction, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let value = vm.world.read_storage(vm.state.current_frame.address, key);
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
