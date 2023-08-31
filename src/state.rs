use crate::{addressing_modes::Arguments, predication::Flags};
use u256::U256;

pub struct State {
    pub program_start: *const Instruction,
    pub program_len: usize,
    pub registers: [U256; 16],
    pub flags: Flags,
    pub stack: Box<[U256; 1 << 16]>,
    pub sp: u16,
    pub code_page: Vec<U256>,
}

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler = fn(&mut State, *const Instruction);

impl Default for State {
    fn default() -> Self {
        Self {
            program_start: std::ptr::null(),
            program_len: 0,
            registers: Default::default(),
            flags: Flags::new(false, false, false),
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            sp: 1000,
            code_page: vec![],
        }
    }
}

impl State {
    pub fn run<'a>(&'a mut self, program: &'a [Instruction]) {
        self.program_start = &program[0];
        self.program_len = program.len();

        let start = program[0].handler;
        start(self, &program[0]);
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
    let first_handler = unsafe { (*state.program_start).handler };
    first_handler(state, state.program_start);
}
