use std::fmt::{self, Debug, Formatter};

use crate::{addressing_modes::Arguments, vm::VirtualMachine, Predicate, World};

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

impl Debug for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Some handler with args {:?}", self.arguments)
    }
}

pub(crate) type Handler =
    fn(&mut VirtualMachine, *const Instruction, &mut dyn World) -> InstructionResult;
pub(crate) type InstructionResult = Result<*const Instruction, ExecutionEnd>;

#[derive(Debug, PartialEq)]
pub enum ExecutionEnd {
    ProgramFinished(Vec<u8>),
    Reverted(Vec<u8>),
    Panicked,

    /// Returned when the bootloader writes to the heap location [crate::Settings::hook_address]
    SuspendedOnHook {
        hook: u32,
        pc_to_resume_from: u16,
    },
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn jump_to_beginning_handler(
    vm: &mut VirtualMachine,
    _: *const Instruction,
    _: &mut dyn World,
) -> InstructionResult {
    let first_instruction = vm.state.current_frame.program.instruction(0).unwrap();
    Ok(first_instruction)
}
