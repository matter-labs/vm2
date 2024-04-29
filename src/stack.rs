use crate::bitset::Bitset;
use u256::U256;

#[derive(Clone, PartialEq, Debug)]
pub struct Stack {
    pub pointer_flags: Bitset,
    pub slots: [U256; 1 << 16],
}

impl Stack {
    pub(crate) fn new() -> Box<Self> {
        Box::new(Stack {
            pointer_flags: Default::default(),
            slots: [U256::zero(); 1 << 16],
        })
    }
    fn zero(&mut self) {
        self.pointer_flags = Default::default();

        // This loop results in just one call to _bzero unlike setting self.slots
        for slot in self.slots.iter_mut() {
            *slot = U256::zero();
        }
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
