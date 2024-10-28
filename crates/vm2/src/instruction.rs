use std::fmt;

pub(crate) use zksync_vm2_interface::ExecutionStatus;

use crate::{addressing_modes::Arguments, vm::VirtualMachine};

/// Single EraVM instruction (an opcode + [`Arguments`]).
///
/// Managing instructions is warranted for low-level tests; prefer using [`Program`](crate::Program)s to decode instructions
/// from EraVM bytecodes.
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
