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

/// End of a VM execution returned from [`VirtualMachine::run()`].
#[derive(Debug, PartialEq)]
pub enum ExecutionEnd {
    /// The executed program has finished and returned the specified data.
    ProgramFinished(Vec<u8>),
    /// The executed program has reverted returning the specified data.
    Reverted(Vec<u8>),
    /// The executed program has panicked.
    Panicked,
    /// Returned when the bootloader writes to the heap location specified by [`hook_address`](crate::Settings.hook_address).
    SuspendedOnHook(u32),
}
