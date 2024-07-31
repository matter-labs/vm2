use std::fmt;

use crate::{
    addressing_modes::Arguments, mode_requirements::ModeRequirements, vm::VirtualMachine,
    Predicate, World,
};

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

impl fmt::Debug for Instruction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Instruction")
            .field("arguments", &self.arguments)
            .finish_non_exhaustive()
    }
}

pub(crate) type Handler = fn(&mut VirtualMachine, &mut dyn World) -> InstructionResult;
pub(crate) type InstructionResult = Option<ExecutionEnd>;

#[derive(Debug, PartialEq)]
pub enum ExecutionEnd {
    ProgramFinished(Vec<u8>),
    Reverted(Vec<u8>),
    Panicked,

    /// Returned when the bootloader writes to the heap location [crate::Settings::hook_address]
    SuspendedOnHook(u32),
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    }
}
fn jump_to_beginning_handler(vm: &mut VirtualMachine, _: &mut dyn World) -> InstructionResult {
    let first_instruction = vm.state.current_frame.program.instruction(0).unwrap();
    vm.state.current_frame.pc = first_instruction;
    None
}
