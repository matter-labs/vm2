use crate::{
    addressing_modes::{Addressable, Arguments},
    bitset::Bitset,
    decommit::{address_into_u256, decommit},
    instruction_handlers,
    modified_world::ModifiedWorld,
    predication::Flags,
    World,
};
use arbitrary::{Arbitrary, Unstructured};
use std::sync::Arc;
use u256::{H160, U256};

pub struct State {
    pub world: ModifiedWorld,

    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe,
    previous_frames: Vec<(*const Instruction, Callframe)>,

    pub(crate) heaps: Vec<Vec<u8>>,
}

pub struct Callframe {
    pub address: H160,
    pub program: Arc<[Instruction]>,
    pub code_page: Arc<[U256]>,

    // TODO: joint allocate these.
    pub stack: Box<[U256; 1 << 16]>,
    pub stack_pointer_flags: Box<Bitset>,
    pub sp: u16,

    pub heap: u32,
    pub aux_heap: u32,

    pub gas: u32,
}

impl Addressable for State {
    fn registers(&mut self) -> &mut [U256; 16] {
        &mut self.registers
    }
    fn register_pointer_flags(&mut self) -> &mut u16 {
        &mut self.register_pointer_flags
    }
    fn stack(&mut self) -> &mut [U256; 1 << 16] {
        &mut self.current_frame.stack
    }
    fn stack_pointer_flags(&mut self) -> &mut Bitset {
        &mut self.current_frame.stack_pointer_flags
    }
    fn stack_pointer(&mut self) -> &mut u16 {
        &mut self.current_frame.sp
    }
    fn code_page(&self) -> &[U256] {
        &self.current_frame.code_page
    }
}

impl Callframe {
    fn new(
        address: H160,
        program: Arc<[Instruction]>,
        code_page: Arc<[U256]>,
        heap: u32,
        aux_heap: u32,
        gas: u32,
    ) -> Self {
        Self {
            address,
            program,
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            stack_pointer_flags: Default::default(),
            sp: 1024,
            code_page,
            heap,
            aux_heap,
            gas,
        }
    }
}

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler = fn(&mut State, *const Instruction) -> ExecutionResult;
pub type ExecutionResult = Result<(), Panic>;

#[derive(Debug)]
pub enum Panic {
    OutOfGas,
    IncorrectPointerTags,
    PointerOffsetTooLarge,
    PtrPackLowBitsNotZero,
    JumpingOutOfProgram,
}

impl State {
    pub fn new(mut world: Box<dyn World>, address: H160, calldata: Vec<u8>) -> Self {
        let (program, code_page) = decommit(&mut *world, address_into_u256(address));
        let mut registers: [U256; 16] = Default::default();
        registers[1] = instruction_handlers::FatPointer {
            memory_page: 0,
            offset: 0,
            start: 0,
            length: calldata.len() as u32,
        }
        .into_u256();
        Self {
            world: ModifiedWorld::new(world),
            registers,
            register_pointer_flags: 1 << 1, // calldata is a pointer
            flags: Flags::new(false, false, false),
            current_frame: Callframe::new(address, program, code_page, 1, 2, 4000),
            previous_frames: vec![],
            heaps: vec![calldata, vec![], vec![]],
        }
    }

    pub(crate) fn push_frame(
        &mut self,
        instruction_pointer: *const Instruction,
        address: H160,
        program: Arc<[Instruction]>,
        code_page: Arc<[U256]>,
        gas: u32,
    ) {
        let new_heap = self.heaps.len() as u32;
        self.heaps.extend([vec![], vec![]]);
        let mut new_frame =
            Callframe::new(address, program, code_page, new_heap, new_heap + 1, gas);

        std::mem::swap(&mut new_frame, &mut self.current_frame);
        self.previous_frames.push((instruction_pointer, new_frame));
    }

    pub fn run(&mut self) -> ExecutionResult {
        let mut instruction: *const Instruction = &self.current_frame.program[0];

        self.use_gas(1)?;

        // Instructions check predication for the *next* instruction, not the current one.
        // Thus, we can't just blindly run the first instruction.
        unsafe {
            while !(*instruction).arguments.predicate.satisfied(&self.flags) {
                instruction = instruction.add(1);
                self.use_gas(1)?;
            }
            ((*instruction).handler)(self, instruction)
        }
    }

    #[inline(always)]
    pub(crate) fn use_gas(&mut self, amount: u32) -> Result<(), Panic> {
        if self.current_frame.gas >= amount {
            self.current_frame.gas -= amount;
            Ok(())
        } else {
            Err(Panic::OutOfGas)
        }
    }
}

pub fn end_execution() -> Instruction {
    Instruction {
        handler: end_execution_handler,
        arguments: Arguments::default(),
    }
}
fn end_execution_handler(_state: &mut State, _: *const Instruction) -> ExecutionResult {
    Ok(())
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::default(),
    }
}
fn jump_to_beginning_handler(state: &mut State, _: *const Instruction) -> ExecutionResult {
    let first_instruction = &state.current_frame.program[0];
    let first_handler = first_instruction.handler;
    first_handler(state, first_instruction)
}

pub fn run_arbitrary_program(input: &[u8]) -> ExecutionResult {
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
        fn decommit(&mut self, hash: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
            todo!()
        }

        fn read_storage(&mut self, _: H160, _: U256) -> U256 {
            U256::zero()
        }
    }

    let mut state = State::new(Box::new(FakeWorld), H160::zero(), vec![]);
    state.run()
}
