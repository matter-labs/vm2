use crate::{
    address_into_u256,
    addressing_modes::Addressable,
    bitset::Bitset,
    callframe::Callframe,
    decommit::{is_kernel, u256_into_address},
    fat_pointer::FatPointer,
    instruction_handlers::CallingMode,
    modified_world::Snapshot,
    predication::Flags,
    program::Program,
    Instruction,
};
use std::ops::{Index, IndexMut};
use u256::{H160, U256};

#[derive(Clone)]
pub struct State {
    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe,

    /// Contains indices to the far call instructions currently being executed.
    /// They are needed to continue execution from the correct spot upon return.
    previous_frames: Vec<(u16, Callframe)>,

    pub heaps: Heaps,

    context_u128: u128,
}

impl State {
    pub(crate) fn new(
        address: H160,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        program: Program,
        world_before_this_frame: Snapshot,
    ) -> Self {
        let mut registers: [U256; 16] = Default::default();
        registers[1] = FatPointer {
            memory_page: 1,
            offset: 0,
            start: 0,
            length: calldata.len() as u32,
        }
        .into_u256();

        Self {
            registers,
            register_pointer_flags: 1 << 1, // calldata is a pointer
            flags: Flags::new(false, false, false),
            current_frame: Callframe::new(
                address,
                address,
                caller,
                program,
                2,
                3,
                1,
                gas,
                0,
                0,
                0,
                false,
                world_before_this_frame,
            ),
            previous_frames: vec![],

            // The first heap can never be used because heap zero
            // means the current heap in precompile calls
            heaps: Heaps(vec![vec![], calldata, vec![], vec![]]),
            context_u128: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn use_gas(&mut self, amount: u32) -> Result<(), ()> {
        if self.current_frame.gas >= amount {
            self.current_frame.gas -= amount;
            Ok(())
        } else {
            self.current_frame.gas = 0;
            Err(())
        }
    }

    /// Returns the total unspent gas in the VM, including stipends.
    pub(crate) fn total_unspent_gas(&self) -> u32 {
        self.current_frame.gas
            + self
                .previous_frames
                .iter()
                .map(|(_, frame)| frame.contained_gas())
                .sum::<u32>()
    }

    pub(crate) fn push_frame<const CALLING_MODE: u8>(
        &mut self,
        instruction_pointer: *const Instruction,
        code_address: H160,
        program: Program,
        gas: u32,
        stipend: u32,
        exception_handler: u16,
        is_static: bool,
        calldata_heap: u32,
        world_before_this_frame: Snapshot,
    ) {
        let new_heap = self.heaps.0.len() as u32;
        let new_heap_len = if is_kernel(address_into_u256(code_address)) {
            zkevm_opcode_defs::system_params::NEW_KERNEL_FRAME_MEMORY_STIPEND
        } else {
            zkevm_opcode_defs::system_params::NEW_FRAME_MEMORY_STIPEND
        } as usize;
        self.heaps
            .0
            .extend([vec![0; new_heap_len], vec![0; new_heap_len]]);

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
            new_heap,
            new_heap + 1,
            calldata_heap,
            gas,
            stipend,
            exception_handler,
            if CALLING_MODE == CallingMode::Delegate as u8 {
                self.current_frame.context_u128
            } else {
                self.context_u128
            },
            is_static || self.current_frame.is_static,
            world_before_this_frame,
        );
        self.context_u128 = 0;

        let old_pc = self.current_frame.pc_to_u16(instruction_pointer);
        std::mem::swap(&mut new_frame, &mut self.current_frame);
        self.previous_frames.push((old_pc, new_frame));
    }

    pub(crate) fn pop_frame(&mut self, heap_to_keep: Option<u32>) -> Option<(u16, u16, Snapshot)> {
        self.previous_frames.pop().map(|(pc, frame)| {
            for &heap in [self.current_frame.heap, self.current_frame.aux_heap]
                .iter()
                .chain(&self.current_frame.heaps_i_am_keeping_alive)
            {
                if Some(heap) != heap_to_keep {
                    self.heaps.deallocate(heap);
                }
            }

            let eh = self.current_frame.exception_handler;
            let snapshot = self.current_frame.world_before_this_frame;

            self.current_frame = frame;
            self.current_frame
                .heaps_i_am_keeping_alive
                .extend(heap_to_keep);

            (pc, eh, snapshot)
        })
    }

    /// Pushes a new frame with only the exception handler set to a sensible value.
    /// Only to be used when far call panics.
    pub(crate) fn push_dummy_frame(
        &mut self,
        instruction_pointer: *const Instruction,
        exception_handler: u16,
        world_before_this_frame: Snapshot,
    ) {
        let mut new_frame = Callframe::new(
            H160::zero(),
            H160::zero(),
            H160::zero(),
            Program::new(vec![], vec![]),
            0,
            0,
            0,
            0,
            0,
            exception_handler,
            0,
            false,
            world_before_this_frame,
        );

        let old_pc = self.current_frame.pc_to_u16(instruction_pointer);
        std::mem::swap(&mut new_frame, &mut self.current_frame);
        self.previous_frames.push((old_pc, new_frame));
    }

    pub(crate) fn set_context_u128(&mut self, value: u128) {
        self.context_u128 = value;
    }

    pub(crate) fn get_context_u128(&self) -> u128 {
        self.current_frame.context_u128
    }
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
        self.current_frame.program.code_page()
    }
}

#[derive(Debug, Clone)]
pub struct Heaps(Vec<Vec<u8>>);

impl Heaps {
    pub(crate) fn deallocate(&mut self, heap: u32) {
        self.0[heap as usize] = vec![];
    }
}

impl Index<u32> for Heaps {
    type Output = Vec<u8>;

    fn index(&self, index: u32) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<u32> for Heaps {
    fn index_mut(&mut self, index: u32) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}
