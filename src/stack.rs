use crate::bitset::Bitset;
use u256::U256;

#[derive(Clone, PartialEq, Debug)]
pub struct Stack {
    pub pointer_flags: Bitset,
    pub slots: [U256; 1 << 16],
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
                s.slots = [U256::zero(); 1 << 16];
                s.pointer_flags = Default::default();
                s
            })
            .unwrap_or_else(|| {
                Box::new(Stack {
                    pointer_flags: Default::default(),
                    slots: [U256::zero(); 1 << 16],
                })
            })
    }

    pub fn recycle(&mut self, stack: Box<Stack>) {
        self.stacks.push(stack);
    }
}
