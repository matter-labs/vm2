use std::fmt;

use crate::{
    addressing_modes::Arguments, mode_requirements::ModeRequirements, vm::VirtualMachine, Predicate,
};

pub struct Instruction<T, W> {
    pub(crate) handler: Handler<T, W>,
    pub(crate) arguments: Arguments,
}

impl<T, W> fmt::Debug for Instruction<T, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Instruction")
            .field("arguments", &self.arguments)
            .finish_non_exhaustive()
    }
}

pub(crate) type Handler<T, W> = fn(&mut VirtualMachine<T, W>, &mut W, &mut T) -> ExecutionStatus;
pub enum ExecutionStatus {
    Running,
    Stopped(ExecutionEnd),
}

#[derive(Debug, PartialEq)]
pub enum ExecutionEnd {
    ProgramFinished(Vec<u8>),
    Reverted(Vec<u8>),
    Panicked,

    /// Returned when the bootloader writes to the heap location [crate::Settings::hook_address]
    SuspendedOnHook(u32),
}

pub fn jump_to_beginning<T, W>() -> Instruction<T, W> {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    }
}
fn jump_to_beginning_handler<T, W>(
    vm: &mut VirtualMachine<T, W>,
    _: &mut W,
    _: &mut T,
) -> ExecutionStatus {
    let first_instruction = vm.state.current_frame.program.instruction(0).unwrap();
    vm.state.current_frame.pc = first_instruction;
    ExecutionStatus::Running
}
