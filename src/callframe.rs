use crate::{
    bitset::Bitset, modified_world::ModifiedWorld, program::Program, rollback::Rollback,
    Instruction,
};
use u256::{H160, U256};

pub struct Callframe {
    pub address: H160,
    pub code_address: H160,
    pub caller: H160,
    pub program: Program,
    pub(crate) exception_handler: u32,
    pub(crate) context_u128: u128,
    pub(crate) is_static: bool,

    // TODO: joint allocate these.
    pub stack: Box<[U256; 1 << 16]>,
    pub stack_pointer_flags: Box<Bitset>,

    pub heap: u32,
    pub aux_heap: u32,

    pub sp: u16,
    pub gas: u32,
    pub stipend: u32,

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

pub(crate) type Snapshot = <ModifiedWorld as Rollback>::Snapshot;

impl Callframe {
    pub(crate) fn new(
        address: H160,
        code_address: H160,
        caller: H160,
        program: Program,
        heap: u32,
        aux_heap: u32,
        gas: u32,
        stipend: u32,
        exception_handler: u32,
        context_u128: u128,
        is_static: bool,
        world_before_this_frame: Snapshot,
    ) -> Self {
        Self {
            address,
            code_address,
            caller,
            program,
            context_u128,
            is_static,
            stack: vec![U256::zero(); 1 << 16]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            stack_pointer_flags: Default::default(),
            heap,
            aux_heap,
            sp: 1024,
            gas,
            stipend,
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

    pub(crate) fn pc_to_u32(&self, pc: *const Instruction) -> u32 {
        unsafe { pc.offset_from(&self.program.instructions()[0]) as u32 }
    }

    pub(crate) fn pc_from_u32(&self, index: u32) -> Option<*const Instruction> {
        self.program
            .instructions()
            .get(index as usize)
            .map(|p| p as *const Instruction)
    }
}
