use crate::bitset::Bitset;
use std::alloc::{alloc_zeroed, Layout};
use u256::U256;

#[derive(Clone, PartialEq, Debug)]
pub struct Stack {
    /// set of slots that may be interpreted as [crate::fat_pointer::FatPointer].
    pub pointer_flags: Bitset,
    dirty_areas: u64,
    slots: [U256; 1 << 16],
}

const NUMBER_OF_DIRTY_AREAS: usize = 64;
const DIRTY_AREA_SIZE: usize = (1 << 16) / NUMBER_OF_DIRTY_AREAS;

impl Stack {
    pub(crate) fn new() -> Box<Self> {
        unsafe { Box::from_raw(alloc_zeroed(Layout::new::<Stack>()).cast::<Stack>()) }
    }

    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.slots[slot as usize]
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256) {
        let written_area = slot as usize / DIRTY_AREA_SIZE;
        self.dirty_areas |= 1 << written_area;

        self.slots[slot as usize] = value;
    }

    fn zero(&mut self) {
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if self.dirty_areas & (1 << i) != 0 {
                for slot in self.slots[i * DIRTY_AREA_SIZE..(i + 1) * DIRTY_AREA_SIZE].iter_mut() {
                    *slot = U256::zero();
                }
            }
        }

        self.dirty_areas = 0;
        self.pointer_flags = Default::default();
    }
}

#[derive(Default)]
pub struct StackPool {
    stacks: Vec<Box<Stack>>,
}

impl StackPool {
    pub fn get(&mut self) -> Box<Stack> {
        self.stacks
            .pop()
            .map(|mut s| {
                s.zero();
                s
            })
            .unwrap_or_else(Stack::new)
    }

    pub fn recycle(&mut self, stack: Box<Stack>) {
        self.stacks.push(stack);
    }
}
