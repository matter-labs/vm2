use crate::{addressing_modes::Arguments, vm::VirtualMachine, Predicate};

#[derive(Hash)]
pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler = fn(&mut VirtualMachine, *const Instruction) -> InstructionResult;
pub(crate) type InstructionResult = Result<*const Instruction, ExecutionEnd>;

#[derive(Debug)]
pub enum ExecutionEnd {
    ProgramFinished(Vec<u8>),
    Reverted(Vec<u8>),
    Panicked,
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn jump_to_beginning_handler(vm: &mut VirtualMachine, _: *const Instruction) -> InstructionResult {
    let first_instruction = &vm.state.current_frame.program.instructions()[0];
    Ok(first_instruction)
}
