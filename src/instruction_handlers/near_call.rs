use crate::{
    addressing_modes::{Arguments, Immediate1, Immediate2, Register1, Source},
    instruction::InstructionResult,
    predication::Flags,
    Instruction, VirtualMachine, World,
};

fn near_call(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    _: &mut dyn World,
) -> InstructionResult {
    let args = unsafe { &(*instruction).arguments };

    let gas_to_pass = Register1::get(args, &mut vm.state).0[0] as u32;
    let destination = Immediate1::get(args, &mut vm.state);
    let error_handler = Immediate2::get(args, &mut vm.state);

    let new_frame_gas = if gas_to_pass == 0 {
        vm.state.current_frame.gas
    } else {
        gas_to_pass.min(vm.state.current_frame.gas)
    };
    vm.state.current_frame.push_near_call(
        new_frame_gas,
        instruction,
        error_handler.low_u32() as u16,
        vm.world_diff.snapshot(),
    );

    vm.state.flags = Flags::new(false, false, false);

    Ok(&vm.state.current_frame.program.instructions()[destination.low_u32() as usize])
}

impl Instruction {
    pub fn from_near_call(
        gas: Register1,
        destination: Immediate1,
        error_handler: Immediate2,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: near_call,
            arguments: arguments
                .write_source(&gas)
                .write_source(&destination)
                .write_source(&error_handler),
        }
    }
}
