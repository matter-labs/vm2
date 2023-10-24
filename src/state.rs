use crate::{
    addressing_modes::Arguments, bitset::Bitset, instruction_handlers, predication::Flags,
};
use arbitrary::{Arbitrary, Unstructured};
use u256::U256;

pub struct State {
    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe,
    previous_frames: Vec<(*const Instruction, Callframe)>,

    pub(crate) heaps: Vec<Vec<u8>>,

    gas: u32,
}

pub struct Callframe {
    pub program_start: *const Instruction,
    pub program_len: usize,
    pub code_page: Vec<U256>,

    // TODO: joint allocate these. Difficult because stack must be on the stack the whole time.
    pub stack: Box<[U256; 1 << 16]>,
    pub stack_pointer_flags: Box<Bitset>,
    pub sp: u16,

    pub heap: u32,
    pub aux_heap: u32,
}

impl Callframe {
    fn new(program: &[Instruction], code_page: Vec<U256>, heap: u32, aux_heap: u32) -> Self {
        const INITIAL_SP: u16 = 1000;

        Self {
            program_start: &program[0],
            program_len: program.len(),
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            stack_pointer_flags: Box::new(Bitset::default()),
            sp: INITIAL_SP,
            code_page,
            heap,
            aux_heap,
        }
    }
}

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler = fn(&mut State, *const Instruction);

impl State {
    pub fn new(program: &[Instruction], code_page: Vec<U256>) -> Self {
        Self {
            registers: Default::default(),
            register_pointer_flags: 0,
            flags: Flags::new(false, false, false),
            current_frame: Callframe::new(program, code_page, 0, 1),
            previous_frames: vec![],
            heaps: vec![vec![], vec![]],
            gas: 4000,
        }
    }

    pub fn run(&mut self) {
        let mut instruction = self.current_frame.program_start;

        if self.use_gas(1) {
            return instruction_handlers::panic();
        }

        // Instructions check predication for the *next* instruction, not the current one.
        // Thus, we can't just blindly run the first instruction.
        unsafe {
            while !(*instruction).arguments.predicate.satisfied(&self.flags) {
                instruction = instruction.add(1);
                if self.use_gas(1) {
                    return instruction_handlers::panic();
                }
            }
            ((*instruction).handler)(self, instruction)
        }
    }

    #[inline(always)]
    pub(crate) fn use_gas(&mut self, amount: u32) -> bool {
        if self.gas >= amount {
            self.gas -= amount;
            false
        } else {
            true
        }
    }
}

pub fn end_execution() -> Instruction {
    Instruction {
        handler: end_execution_handler,
        arguments: Arguments::default(),
    }
}
fn end_execution_handler(_state: &mut State, _: *const Instruction) {}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::default(),
    }
}
fn jump_to_beginning_handler(state: &mut State, _: *const Instruction) {
    let first_handler = unsafe { (*state.current_frame.program_start).handler };
    first_handler(state, state.current_frame.program_start);
}

pub fn run_arbitrary_program(input: &[u8]) {
    let mut u = Unstructured::new(&input);
    let mut program: Vec<Instruction> = Arbitrary::arbitrary(&mut u).unwrap();

    if program.len() >= 1 << 16 {
        program.truncate(1 << 16);
        program.push(jump_to_beginning());
    } else {
        // TODO execute invalid instruction or something instead
        program.push(end_execution());
    }

    let mut state = State::new(&program, vec![]);
    state.run();
}
