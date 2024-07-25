use crate::{
    decommit::is_kernel,
    heap::HeapId,
    program::Program,
    stack::{Stack, StackSnapshot},
    world_diff::Snapshot,
    Instruction,
};
use u256::H160;
use zkevm_opcode_defs::system_params::{NEW_FRAME_MEMORY_STIPEND, NEW_KERNEL_FRAME_MEMORY_STIPEND};

#[derive(Clone, PartialEq, Debug)]
pub struct Callframe {
    pub address: H160,
    pub code_address: H160,
    pub caller: H160,

    pub exception_handler: u16,
    pub context_u128: u128,
    pub is_static: bool,
    pub is_kernel: bool,

    pub stack: Box<Stack>,
    pub sp: u16,

    pub gas: u32,
    pub stipend: u32,

    pub(crate) near_calls: Vec<NearCallFrame>,

    pub(crate) program: Program,

    pub heap: HeapId,
    pub aux_heap: HeapId,

    /// The amount of heap that has been paid for. This should always be greater
    /// or equal to the actual size of the heap in memory.
    pub heap_size: u32,
    pub aux_heap_size: u32,

    /// Returning a pointer to the calldata is illegal because it could result in
    /// the caller's heap being accessible both directly and via the fat pointer.
    /// The problem only occurs if the calldata originates from the caller's heap
    /// but this rule is easy to implement.
    pub(crate) calldata_heap: HeapId,

    /// Because of the above rule we know that heaps returned to this frame only
    /// exist to allow this frame to read from them. Therefore we can deallocate
    /// all of them upon return, except possibly one that we pass on.
    pub(crate) heaps_i_am_keeping_alive: Vec<HeapId>,

    pub(crate) world_before_this_frame: Snapshot,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct NearCallFrame {
    pub(crate) call_instruction: u16,
    pub(crate) exception_handler: u16,
    pub(crate) previous_frame_sp: u16,
    pub(crate) previous_frame_gas: u32,
    world_before_this_frame: Snapshot,
}

impl Callframe {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        address: H160,
        code_address: H160,
        caller: H160,
        program: Program,
        stack: Box<Stack>,
        heap: HeapId,
        aux_heap: HeapId,
        calldata_heap: HeapId,
        gas: u32,
        stipend: u32,
        exception_handler: u16,
        context_u128: u128,
        is_static: bool,
        world_before_this_frame: Snapshot,
    ) -> Self {
        let is_kernel = is_kernel(address);
        let heap_size = if is_kernel {
            NEW_KERNEL_FRAME_MEMORY_STIPEND
        } else {
            NEW_FRAME_MEMORY_STIPEND
        };

        Self {
            address,
            code_address,
            caller,
            program,
            context_u128,
            is_static,
            is_kernel,
            stack,
            heap,
            aux_heap,
            heap_size,
            aux_heap_size: heap_size,
            calldata_heap,
            heaps_i_am_keeping_alive: vec![],
            sp: 0,
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

    pub(crate) fn pop_near_call(&mut self) -> Option<FrameRemnant> {
        self.near_calls.pop().map(|f| {
            self.sp = f.previous_frame_sp;
            self.gas = f.previous_frame_gas;

            FrameRemnant {
                program_counter: f.call_instruction,
                exception_handler: f.exception_handler,
                snapshot: f.world_before_this_frame,
            }
        })
    }

    pub(crate) fn pc_to_u16(&self, pc: *const Instruction) -> u16 {
        unsafe { pc.offset_from(self.program.instruction(0).unwrap()) as u16 }
    }

    pub fn pc_from_u16(&self, index: u16) -> Option<*const Instruction> {
        self.program
            .instruction(index)
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

    pub(crate) fn snapshot(&self) -> CallframeSnapshot {
        CallframeSnapshot {
            stack: self.stack.snapshot(),

            context_u128: self.context_u128,
            sp: self.sp,
            gas: self.gas,
            near_calls: self.near_calls.clone(),
            heap_size: self.heap_size,
            aux_heap_size: self.aux_heap_size,
            heaps_i_am_keeping_alive: self.heaps_i_am_keeping_alive.clone(),
            world_before_this_frame: self.world_before_this_frame.clone(),
        }
    }

    pub(crate) fn rollback(&mut self, snapshot: CallframeSnapshot) {
        let CallframeSnapshot {
            stack,
            context_u128,
            sp,
            gas,
            near_calls,
            heap_size,
            aux_heap_size,
            heaps_i_am_keeping_alive,
            world_before_this_frame,
        } = snapshot;

        self.stack.rollback(stack);

        self.context_u128 = context_u128;
        self.sp = sp;
        self.gas = gas;
        self.near_calls = near_calls;
        self.heap_size = heap_size;
        self.aux_heap_size = aux_heap_size;
        self.heaps_i_am_keeping_alive = heaps_i_am_keeping_alive;
        self.world_before_this_frame = world_before_this_frame;
    }
}

pub(crate) struct FrameRemnant {
    pub(crate) program_counter: u16,
    pub(crate) exception_handler: u16,
    pub(crate) snapshot: Snapshot,
}

/// Only contains the fields that can change (other than via tracer).
pub(crate) struct CallframeSnapshot {
    stack: StackSnapshot,

    context_u128: u128,
    sp: u16,
    gas: u32,
    near_calls: Vec<NearCallFrame>,

    heap_size: u32,
    aux_heap_size: u32,

    heaps_i_am_keeping_alive: Vec<HeapId>,
    world_before_this_frame: Snapshot,
}
