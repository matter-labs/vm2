use crate::{
    addressing_modes::{Addressable, Arguments},
    bitset::Bitset,
    decommit::{address_into_u256, decommit, u256_into_address},
    fat_pointer::FatPointer,
    instruction_handlers::{ret_panic, CallingMode},
    modified_world::ModifiedWorld,
    predication::Flags,
    rollback::Rollback,
    Predicate, World,
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

    /// Contains indices to the far call instructions currently being executed.
    /// They are needed to continue execution from the correct spot upon return.
    previous_frames: Vec<(u32, Callframe)>,

    pub(crate) heaps: Vec<Vec<u8>>,

    context_u128: u128,
}

type Snapshot = <ModifiedWorld as Rollback>::Snapshot;
pub struct Callframe {
    pub address: H160,
    pub code_address: H160,
    pub caller: H160,
    pub program: Arc<[Instruction]>,
    pub code_page: Arc<[U256]>,
    exception_handler: u32,
    context_u128: u128,

    // TODO: joint allocate these.
    pub stack: Box<[U256; 1 << 16]>,
    pub stack_pointer_flags: Box<Bitset>,

    pub heap: u32,
    pub aux_heap: u32,

    pub sp: u16,
    pub gas: u32,

    near_calls: Vec<NearCallFrame>,

    pub(crate) world_before_this_frame: Snapshot,
}

struct NearCallFrame {
    call_instruction: u32,
    exception_handler: u32,
    previous_frame_sp: u16,
    previous_frame_gas: u32,
    world_before_this_frame: Snapshot,
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
        code_address: H160,
        caller: H160,
        program: Arc<[Instruction]>,
        code_page: Arc<[U256]>,
        heap: u32,
        aux_heap: u32,
        gas: u32,
        exception_handler: u32,
        context_u128: u128,
        world_before_this_frame: Snapshot,
    ) -> Self {
        Self {
            address,
            code_address,
            caller,
            program,
            context_u128,
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            stack_pointer_flags: Default::default(),
            code_page,
            heap,
            aux_heap,
            sp: 1024,
            gas,
            exception_handler,
            near_calls: vec![],
            world_before_this_frame,
        }
    }

    pub(crate) fn push_near_call(
        &mut self,
        gas_to_call: u32,
        old_pc: *const Instruction,
        exception_handler: u32,
        world_before_this_frame: Snapshot,
    ) {
        self.near_calls.push(NearCallFrame {
            call_instruction: self.pc_to_u32(old_pc),
            exception_handler,
            previous_frame_sp: self.sp,
            previous_frame_gas: self.gas - gas_to_call,
            world_before_this_frame,
        });
        self.gas = gas_to_call;
    }

    pub(crate) fn pop_near_call(&mut self) -> Option<(u32, u32, Snapshot)> {
        self.near_calls.pop().map(|f| {
            self.sp = f.previous_frame_sp;
            self.gas = f.previous_frame_gas;
            (
                f.call_instruction,
                f.exception_handler,
                f.world_before_this_frame,
            )
        })
    }

    fn pc_to_u32(&self, pc: *const Instruction) -> u32 {
        unsafe { pc.offset_from(&self.program[0]) as u32 }
    }

    pub(crate) fn pc_from_u32(&self, index: u32) -> Option<*const Instruction> {
        self.program
            .get(index as usize)
            .map(|p| p as *const Instruction)
    }
}

pub struct Instruction {
    pub(crate) handler: Handler,
    pub(crate) arguments: Arguments,
}

pub(crate) type Handler = fn(&mut State, *const Instruction) -> InstructionResult;
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
    CallingCodeThatIsNotYetConstructed,
    AccessingTooLargeHeapAddress,
    InvalidInstruction,
}

impl State {
    pub fn new(
        mut world: Box<dyn World>,
        address: H160,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
    ) -> Self {
        let (program, code_page, _) = decommit(&mut *world, address_into_u256(address)).unwrap();
        let mut registers: [U256; 16] = Default::default();
        registers[1] = FatPointer {
            memory_page: 1,
            offset: 0,
            start: 0,
            length: calldata.len() as u32,
        }
        .into_u256();

        let world = ModifiedWorld::new(world);
        let world_before_this_frame = world.snapshot();

        Self {
            world,
            registers,
            register_pointer_flags: 1 << 1, // calldata is a pointer
            flags: Flags::new(false, false, false),
            current_frame: Callframe::new(
                address,
                address,
                caller,
                program,
                code_page,
                2,
                3,
                gas,
                0,
                0,
                world_before_this_frame,
            ),
            previous_frames: vec![],

            // The first heap can never be used because heap zero
            // means the current heap in precompile calls
            heaps: vec![vec![], calldata, vec![], vec![]],
            context_u128: 0,
        }
    }

    pub(crate) fn push_frame<const CALLING_MODE: u8>(
        &mut self,
        instruction_pointer: *const Instruction,
        code_address: H160,
        program: Arc<[Instruction]>,
        code_page: Arc<[U256]>,
        gas: u32,
        exception_handler: u32,
    ) {
        let new_heap = self.heaps.len() as u32;
        self.heaps.extend([vec![], vec![]]);
        let mut new_frame = Callframe::new(
            if CALLING_MODE == CallingMode::Delegate as u8 {
                self.current_frame.address
            } else {
                code_address
            },
            code_address,
            if CALLING_MODE == CallingMode::Normal as u8 {
                self.current_frame.address
            } else if CALLING_MODE == CallingMode::Delegate as u8 {
                self.current_frame.caller
            } else {
                // Mimic call
                u256_into_address(self.registers[15])
            },
            program,
            code_page,
            new_heap,
            new_heap + 1,
            gas,
            exception_handler,
            if CALLING_MODE == CallingMode::Delegate as u8 {
                self.current_frame.context_u128
            } else {
                self.context_u128
            },
            self.world.snapshot(),
        );
        self.context_u128 = 0;

        let old_pc = self.current_frame.pc_to_u32(instruction_pointer);
        std::mem::swap(&mut new_frame, &mut self.current_frame);
        self.previous_frames.push((old_pc, new_frame));
    }

    pub(crate) fn pop_frame(&mut self) -> Option<(u32, u32, Snapshot)> {
        self.previous_frames.pop().map(|(pc, frame)| {
            let eh = self.current_frame.exception_handler;
            let snapshot = self.current_frame.world_before_this_frame;
            self.current_frame = frame;
            (pc, eh, snapshot)
        })
    }

    pub(crate) fn set_context_u128(&mut self, value: u128) {
        self.context_u128 = value;
    }

    pub(crate) fn get_context_u128(&self) -> u128 {
        self.current_frame.context_u128
    }

    pub fn run(&mut self) -> ExecutionEnd {
        let mut instruction: *const Instruction = &self.current_frame.program[0];

        unsafe {
            loop {
                #[cfg(trace)]
                {
                    print!(
                        "{:?}: ",
                        instruction.offset_from(&self.current_frame.program[0])
                    );
                    self.registers[1..].iter().for_each(|x| print!("{x:?} "));
                    println!();
                }

                let args = &(*instruction).arguments;
                let Ok(_) = self.use_gas(args.get_static_gas_cost()) else {
                    instruction = match ret_panic(self, Panic::OutOfGas) {
                        Ok(i) => i,
                        Err(e) => return e,
                    };
                    continue;
                };

                if args.predicate.satisfied(&self.flags) {
                    instruction = match ((*instruction).handler)(self, instruction) {
                        Ok(n) => n,
                        Err(e) => return e,
                    };
                } else {
                    instruction = instruction.add(1);
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn use_gas(&mut self, amount: u32) -> Result<(), Panic> {
        if self.current_frame.gas >= amount {
            self.current_frame.gas -= amount;
            Ok(())
        } else {
            self.current_frame.gas = 0;
            Err(Panic::OutOfGas)
        }
    }
}

pub fn end_execution() -> Instruction {
    Instruction {
        handler: end_execution_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn end_execution_handler(state: &mut State, _: *const Instruction) -> InstructionResult {
    ret_panic(state, Panic::InvalidInstruction)
}

pub fn jump_to_beginning() -> Instruction {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0),
    }
}
fn jump_to_beginning_handler(state: &mut State, _: *const Instruction) -> InstructionResult {
    let first_instruction = &state.current_frame.program[0];
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
        fn decommit(&mut self, hash: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
            todo!()
        }

        fn read_storage(&mut self, _: H160, _: U256) -> U256 {
            U256::zero()
        }
    }

    let mut state = State::new(
        Box::new(FakeWorld),
        H160::zero(),
        H160::zero(),
        vec![],
        u32::MAX,
    );
    state.run()
}
