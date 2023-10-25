use crate::{
    addressing_modes::{Addressable, Arguments},
    bitset::Bitset,
    instruction_handlers,
    predication::Flags,
    World,
};
use arbitrary::{Arbitrary, Unstructured};
use std::sync::Arc;
use u256::U256;

pub struct State<W: World> {
    pub world: W,

    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe<W>,
    previous_frames: Vec<(*const Instruction<W>, Callframe<W>)>,

    pub(crate) heaps: Vec<Vec<u8>>,
}

pub struct Callframe<W: World> {
    pub program: Arc<[Instruction<W>]>,
    pub code_page: Arc<[U256]>,

    // TODO: joint allocate these.
    pub stack: Box<[U256; 1 << 16]>,
    pub stack_pointer_flags: Box<Bitset>,
    pub sp: u16,

    pub heap: u32,
    pub aux_heap: u32,

    pub gas: u32,
}

impl<W: World> Addressable for State<W> {
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

impl<W: World> Callframe<W> {
    fn new(
        program: Arc<[Instruction<W>]>,
        code_page: Arc<[U256]>,
        heap: u32,
        aux_heap: u32,
        gas: u32,
    ) -> Self {
        const INITIAL_SP: u16 = 1000;

        Self {
            program,
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            stack_pointer_flags: Box::new(Bitset::default()),
            sp: INITIAL_SP,
            code_page,
            heap,
            aux_heap,
            gas,
        }
    }
}

pub struct Instruction<W: World> {
    pub(crate) handler: Handler<W>,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler<W> = fn(&mut State<W>, *const Instruction<W>);

impl<W> State<W>
where
    W: World,
{
    pub fn new(world: W, program: Arc<[Instruction<W>]>, code_page: Arc<[U256]>) -> Self {
        Self {
            world,
            registers: Default::default(),
            register_pointer_flags: 0,
            flags: Flags::new(false, false, false),
            current_frame: Callframe::new(program, code_page, 0, 1, 4000),
            previous_frames: vec![],
            heaps: vec![vec![], vec![]],
        }
    }

    pub(crate) fn push_frame(
        &mut self,
        instruction_pointer: *const Instruction<W>,
        program: Arc<[Instruction<W>]>,
        code_page: Arc<[U256]>,
        gas: u32,
    ) {
        let new_heap = self.heaps.len() as u32;
        self.heaps.extend([vec![], vec![]]);
        let mut new_frame = Callframe::new(program, code_page, new_heap, new_heap + 1, gas);

        std::mem::swap(&mut new_frame, &mut self.current_frame);
        self.previous_frames.push((instruction_pointer, new_frame));
    }

    pub fn run(&mut self) {
        let mut instruction: *const Instruction<W> = &self.current_frame.program[0];

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
        if self.current_frame.gas >= amount {
            self.current_frame.gas -= amount;
            false
        } else {
            true
        }
    }
}

pub fn end_execution<W: World>() -> Instruction<W> {
    Instruction {
        handler: end_execution_handler,
        arguments: Arguments::default(),
    }
}
fn end_execution_handler<W: World>(_state: &mut State<W>, _: *const Instruction<W>) {}

pub fn jump_to_beginning<W: World>() -> Instruction<W> {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::default(),
    }
}
fn jump_to_beginning_handler<W: World>(state: &mut State<W>, _: *const Instruction<W>) {
    let first_instruction = &state.current_frame.program[0];
    let first_handler = first_instruction.handler;
    first_handler(state, first_instruction);
}

pub fn run_arbitrary_program(input: &[u8]) {
    let mut u = Unstructured::new(&input);
    let mut program: Vec<Instruction<FakeWorld>> = Arbitrary::arbitrary(&mut u).unwrap();

    if program.len() >= 1 << 16 {
        program.truncate(1 << 16);
        program.push(jump_to_beginning());
    } else {
        // TODO execute invalid instruction or something instead
        program.push(end_execution());
    }

    struct FakeWorld;
    impl World for FakeWorld {
        fn decommit(&mut self) -> (Arc<[Instruction<Self>]>, Arc<[U256]>) {
            todo!()
        }

        fn read_storage() -> U256 {
            todo!()
        }
    }

    let mut state = State::new(FakeWorld, program.into(), Arc::new([]));
    state.run();
}
