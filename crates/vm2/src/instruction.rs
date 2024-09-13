use std::fmt;

use crate::{addressing_modes::Arguments, vm::VirtualMachine};

#[doc(hidden)] // should only be used for low-level testing / benchmarking
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

#[derive(Debug)]
pub(crate) enum ExecutionStatus {
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
