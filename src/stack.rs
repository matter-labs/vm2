use std::collections::HashMap;
use u256::U256;

#[derive(Debug, Clone, PartialEq)]
struct StackPage {
    words: Box<[U256; 128]>,
    pointer_bitset: u128,
}

impl Default for StackPage {
    fn default() -> Self {
        let words: Box<[U256]> = vec![U256::zero(); 128].into();
        Self {
            words: words.try_into().unwrap(),
            pointer_bitset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    pages: HashMap<usize, StackPage>,
}

impl Stack {
    // Preallocate the first page in any case.
    fn new() -> Self {
        Self {
            pages: HashMap::from([(0, StackPage::default())]),
        }
    }

    pub(crate) fn get(&self, slot: u16) -> U256 {
        let slot = slot as usize;
        let (page_idx, offset_on_page) = (slot >> 7, slot & 127);
        if let Some(page) = self.pages.get(&page_idx) {
            page.words[offset_on_page]
        } else {
            U256::zero()
        }
    }

    pub(crate) fn get_with_pointer_flag(&self, slot: u16) -> (U256, bool) {
        let slot = slot as usize;
        let (page_idx, offset_on_page) = (slot >> 7, slot & 127);
        if let Some(page) = self.pages.get(&page_idx) {
            let bitmask = 1_u128 << offset_on_page;
            (
                page.words[offset_on_page],
                page.pointer_bitset & bitmask != 0,
            )
        } else {
            (U256::zero(), false)
        }
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256, is_pointer: bool) {
        let slot = slot as usize;
        let (page_idx, offset_on_page) = (slot >> 7, slot & 127);
        let page = self.pages.entry(page_idx).or_default();
        page.words[offset_on_page] = value;

        let bitmask = 1_u128 << offset_on_page;
        if is_pointer {
            page.pointer_bitset |= bitmask;
        } else {
            page.pointer_bitset &= !bitmask;
        }
    }
}

#[derive(Default)]
pub struct StackPool {
    _data: (),
}

impl StackPool {
    pub fn get(&mut self) -> Stack {
        Stack::new()
    }

    pub fn recycle(&mut self, _stack: Stack) {
        // Does nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The code produced by derive(Clone) overflows the stack in debug mode.
    #[test]
    fn clone_does_not_segfault() {
        let stack = Stack::new();
        let _ = stack.clone();
    }

    #[test]
    fn basic_stack_operations() {
        let mut stack = Stack::new();
        for slot in [0, 1, 10, 127, 128, 256, 1_000, u16::MAX - 200, u16::MAX] {
            assert_eq!(stack.get(slot), U256::zero());
            assert_eq!(stack.get_with_pointer_flag(slot), (U256::zero(), false));
        }

        for slot in [0, 1, 10, 127, 128, 256, 1_000, u16::MAX - 200, u16::MAX] {
            let value = U256::from(slot);
            stack.set(slot, value, slot % 2 == 0);
            assert_eq!(stack.get(slot), value);
            assert_eq!(stack.get_with_pointer_flag(slot), (value, slot % 2 == 0));
        }
    }
}
