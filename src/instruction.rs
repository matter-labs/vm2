use crate::{
    addressing_modes::Arguments,
    instruction_handlers::ret_panic,
    state::State,
    vm::{Settings, VirtualMachine},
    Predicate, World,
};
use arbitrary::{Arbitrary, Unstructured};
use std::sync::Arc;
use u256::{H160, U256};

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
    Panicked(Panic),
}

#[derive(Debug)]
pub enum Panic {
    ExplicitPanic,
    OutOfGas,
    IncorrectPointerTags,
    PointerOffsetTooLarge,
    PtrPackLowBitsNotZero,
    PointerUpperBoundOverflows,
    PointerOffsetNotZeroAtCreation,
    PointerOffsetOverflows,
    MalformedCodeInfo,
    ConstructorCallAndCodeStatusMismatch,
    AccessingTooLargeHeapAddress,
    WriteInStaticCall,
    InvalidInstruction,
}

pub fn end_execution() -> Instruction {
    Instruction {
        handler: end_execution_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn end_execution_handler(vm: &mut VirtualMachine, _: *const Instruction) -> InstructionResult {
    ret_panic(vm, Panic::InvalidInstruction)
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn jump_to_beginning_handler(vm: &mut VirtualMachine, _: *const Instruction) -> InstructionResult {
    let first_instruction = &vm.state.current_frame.program[0];
    Ok(first_instruction)
}

pub fn run_arbitrary_program(input: &[u8]) -> ExecutionEnd {
    let mut u = Unstructured::new(input);
    let mut program: Vec<Instruction> = Arbitrary::arbitrary(&mut u).unwrap();

    if program.len() >= 1 << 16 {
        program.truncate(1 << 16);
        program.push(jump_to_beginning());
    } else {
        // TODO execute invalid instruction or something instead
        program.push(end_execution());
    }

    struct FakeWorld;
    impl World for FakeWorld {
        fn decommit(&mut self, _hash: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
            todo!()
        }

        fn read_storage(&mut self, _: H160, _: U256) -> U256 {
            U256::zero()
        }

        fn handle_hook(&mut self, _hook: u32, _state: &mut State) {
            todo!()
        }
    }

    let mut state = VirtualMachine::new(
        Box::new(FakeWorld),
        H160::zero(),
        H160::zero(),
        vec![],
        u32::MAX,
        Settings {
            default_aa_code_hash: U256::zero(),
            hook_address: 0,
        },
    );
    state.run()
}
