use crate::{
    addressing_modes::Addressable,
    bitset::Bitset,
    callframe::{self, Callframe, Snapshot},
    decommit::u256_into_address,
    fat_pointer::FatPointer,
    instruction::Panic,
    instruction_handlers::CallingMode,
    predication::Flags,
    Instruction,
};
use std::{
    ops::{Index, IndexMut},
    sync::Arc,
};
use u256::{H160, U256};

pub struct State {
    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe,

    /// Contains indices to the far call instructions currently being executed.
    /// They are needed to continue execution from the correct spot upon return.
    previous_frames: Vec<(u32, Callframe)>,

    pub(crate) heaps: Heaps,

    context_u128: u128,
}

impl State {
    pub(crate) fn new(
        address: H160,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        program: std::sync::Arc<[crate::Instruction]>,
        code_page: std::sync::Arc<[U256]>,
        world_before_this_frame: callframe::Snapshot,
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
                code_page,
                2,
                3,
                gas,
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
    pub(crate) fn use_gas(&mut self, amount: u32) -> Result<(), Panic> {
        if self.current_frame.gas >= amount {
            self.current_frame.gas -= amount;
            Ok(())
        } else {
            self.current_frame.gas = 0;
            Err(Panic::OutOfGas)
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
        is_static: bool,
        world_before_this_frame: Snapshot,
    ) {
        let new_heap = self.heaps.0.len() as u32;
        self.heaps.0.extend([vec![], vec![]]);
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
            is_static || self.current_frame.is_static,
            world_before_this_frame,
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

    pub(crate) fn set_context_u128(&mut self, value: u128) -> Result<(), Panic> {
        if self.current_frame.is_static {
            return Err(Panic::WriteInStaticCall);
        }
        self.context_u128 = value;
        Ok(())
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
        &self.current_frame.code_page
    }
}

#[derive(Debug)]
pub(crate) struct Heaps(Vec<Vec<u8>>);

impl Index<usize> for Heaps {
    type Output = Vec<u8>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<usize> for Heaps {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}
