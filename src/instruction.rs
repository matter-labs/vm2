use std::fmt;

use crate::{
    addressing_modes::Arguments, mode_requirements::ModeRequirements, vm::VirtualMachine,
    Predicate, World,
};

pub struct Instruction<T> {
    pub(crate) handler: Handler<T>,
    pub(crate) arguments: Arguments,
}

impl<T> fmt::Debug for Instruction<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Instruction")
            .field("arguments", &self.arguments)
            .finish_non_exhaustive()
    }
}

pub(crate) type Handler<T> =
    fn(&mut VirtualMachine<T>, &mut dyn World<T>, &mut T) -> InstructionResult;
pub(crate) type InstructionResult = Option<ExecutionEnd>;

#[derive(Debug, PartialEq)]
pub enum ExecutionEnd {
    ProgramFinished(Vec<u8>),
    Reverted(Vec<u8>),
    Panicked,

    /// Returned when the bootloader writes to the heap location [crate::Settings::hook_address]
    SuspendedOnHook(u32),
}

pub fn jump_to_beginning<T>() -> Instruction<T> {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    }
}
fn jump_to_beginning_handler<T>(
    vm: &mut VirtualMachine<T>,
    _: &mut dyn World<T>,
    _: &mut T,
) -> InstructionResult {
    let first_instruction = vm.state.current_frame.program.instruction(0).unwrap();
    vm.state.current_frame.pc = first_instruction;
    None
}
