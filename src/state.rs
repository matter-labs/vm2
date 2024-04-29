use crate::{
    addressing_modes::Addressable, bitset::Bitset, callframe::Callframe, fat_pointer::FatPointer,
    modified_world::Snapshot, predication::Flags, program::Program, stack::Stack,
};
use std::ops::{Index, IndexMut};
use u256::{H160, U256};

#[derive(Clone, PartialEq, Debug)]
pub struct State {
    pub registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,

    pub flags: Flags,

    pub current_frame: Callframe,

    /// Contains indices to the far call instructions currently being executed.
    /// They are needed to continue execution from the correct spot upon return.
    pub previous_frames: Vec<(u16, Callframe)>,

    pub heaps: Heaps,

    pub transaction_number: u16,

    pub(crate) context_u128: u128,
}

pub const FIRST_HEAP: u32 = 2;

impl State {
    pub(crate) fn new(
        address: H160,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        program: Program,
        world_before_this_frame: Snapshot,
        stack: Box<Stack>,
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
                stack,
                FIRST_HEAP,
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

            transaction_number: 0,
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

    fn read_stack(&mut self, slot: u16) -> U256 {
        self.current_frame.stack.get(slot)
    }
    fn write_stack(&mut self, slot: u16, value: U256) {
        self.current_frame.stack.set(slot, value)
    }
    fn stack_pointer_flags(&mut self) -> &mut Bitset {
        &mut self.current_frame.stack.pointer_flags
    }
    fn stack_pointer(&mut self) -> &mut u16 {
        &mut self.current_frame.sp
    }
    fn code_page(&self) -> &[U256] {
        self.current_frame.program.code_page()
    }
}

#[derive(Debug, Clone)]
pub struct Heaps(pub(crate) Vec<Vec<u8>>);

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

impl PartialEq for Heaps {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..self.0.len().max(other.0.len()) {
            if self.0.get(i).unwrap_or(&vec![]) != other.0.get(i).unwrap_or(&vec![]) {
                return false;
            }
        }
        true
    }
}
