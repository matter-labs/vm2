use crate::{modified_world::Snapshot, program::Program, stack::Stack, Instruction};
use u256::H160;

#[derive(Clone, PartialEq, Debug)]
pub struct Callframe {
    pub address: H160,
    pub code_address: H160,
    pub caller: H160,

    pub exception_handler: u16,
    pub context_u128: u128,
    pub is_static: bool,

    // TODO: joint allocate these.
    pub stack: Box<Stack>,

    pub heap: u32,
    pub aux_heap: u32,
    pub sp: u16,

    pub gas: u32,
    pub stipend: u32,

    near_calls: Vec<NearCallFrame>,

    pub(crate) program: Program,

    /// Returning a pointer to the calldata is illegal because it could result in
    /// the caller's heap being accessible both directly and via the fat pointer.
    /// The problem only occurs if the calldata originates from the caller's heap
    /// but this rule is easy to implement.
    pub(crate) calldata_heap: u32,

    /// Because of the above rule we know that heaps returned to this frame only
    /// exist to allow this frame to read from them. Therefore we can deallocate
    /// all of them upon return, except possibly one that we pass on.
    pub(crate) heaps_i_am_keeping_alive: Vec<u32>,

    pub(crate) world_before_this_frame: Snapshot,
}

#[derive(Clone, PartialEq, Debug)]
struct NearCallFrame {
    call_instruction: u16,
    exception_handler: u16,
    previous_frame_sp: u16,
    previous_frame_gas: u32,
    world_before_this_frame: Snapshot,
}

impl Callframe {
    pub(crate) fn new(
        address: H160,
        code_address: H160,
        caller: H160,
        program: Program,
        stack: Box<Stack>,
        heap: u32,
        aux_heap: u32,
        calldata_heap: u32,
        gas: u32,
        stipend: u32,
        exception_handler: u16,
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
            stack,
            heap,
            aux_heap,
            calldata_heap,
            heaps_i_am_keeping_alive: vec![],
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
        exception_handler: u16,
        world_before_this_frame: Snapshot,
    ) {
        self.near_calls.push(NearCallFrame {
            call_instruction: self.pc_to_u16(old_pc),
            exception_handler,
            previous_frame_sp: self.sp,
            previous_frame_gas: self.gas - gas_to_call,
            world_before_this_frame,
        });
        self.gas = gas_to_call;
    }

    pub(crate) fn pop_near_call(&mut self) -> Option<(u16, u16, Snapshot)> {
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

    pub(crate) fn pc_to_u16(&self, pc: *const Instruction) -> u16 {
        unsafe { pc.offset_from(&self.program.instructions()[0]) as u16 }
    }

    pub(crate) fn pc_from_u16(&self, index: u16) -> Option<*const Instruction> {
        self.program
            .instructions()
            .get(index as usize)
            .map(|p| p as *const Instruction)
    }

    /// The total amount of gas in this frame, including gas currently inaccessible because of a near call.
    pub(crate) fn contained_gas(&self) -> u32 {
        self.gas
            + self
                .near_calls
                .iter()
                .map(|f| f.previous_frame_gas)
                .sum::<u32>()
    }
}
