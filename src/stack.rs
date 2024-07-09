use crate::bitset::Bitset;
use u256::U256;

#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    words: Vec<U256>,
    pointer_bitset: Box<Bitset>,
}

impl Stack {
    /// Number of pre-allocated words in the stack.
    const INITIAL_WORD_CAPACITY: usize = 128; // 4kB

    fn new() -> Self {
        Self {
            words: vec![U256::zero(); Self::INITIAL_WORD_CAPACITY],
            pointer_bitset: Box::default(),
        }
    }

    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.words.get(slot as usize).copied().unwrap_or_default()
    }

    pub(crate) fn get_with_pointer_flag(&self, slot: u16) -> (U256, bool) {
        let value = self.words.get(slot as usize).copied().unwrap_or_default();
        (value, self.pointer_bitset.get(slot))
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256, is_pointer: bool) {
        if is_pointer {
            self.pointer_bitset.set(slot);
        } else {
            self.pointer_bitset.clear(slot);
        }

        let slot = slot as usize;
        if self.words.len() <= slot {
            self.words.resize(slot + 1, U256::zero());
        }
        self.words[slot] = value;
    }

    fn zero(&mut self) {
        self.words.fill(U256::zero());
        self.pointer_bitset = Box::default();
    }
}

#[derive(Default)]
pub struct StackPool {
    stacks: Vec<Stack>,
}

impl StackPool {
    pub fn get(&mut self) -> Stack {
        self.stacks
            .pop()
            .map(|mut stack| {
                stack.zero();
                stack
            })
            .unwrap_or_else(Stack::new)
    }

    pub fn recycle(&mut self, mut stack: Stack) {
        // We don't want to have large stacks reused because zeroizing them would require non-trivial effort.
        const MAX_STACK_CAPACITY_TO_REUSE: usize = 1_024; // 32kB

        stack.words.truncate(MAX_STACK_CAPACITY_TO_REUSE);
        stack.words.shrink_to(MAX_STACK_CAPACITY_TO_REUSE);
        self.stacks.push(stack);
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
