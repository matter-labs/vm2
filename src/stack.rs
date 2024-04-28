use crate::bitset::Bitset;
use std::sync::mpsc;
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

const CLEANERS: usize = 4;

pub struct StackPool {
    clean_stack_source: mpsc::Receiver<Box<Stack>>,
    dirty_stack_sinks: [mpsc::Sender<Box<Stack>>; CLEANERS],
    current_cleaner: usize,
}

impl Default for StackPool {
    fn default() -> Self {
        let (clean_stack_sink, clean_stack_source) = mpsc::channel();

        Self {
            clean_stack_source,
            current_cleaner: 0,

            dirty_stack_sinks: (0..CLEANERS)
                .map(|_| {
                    let (dirty_stack_sink, dirty_stack_source) = mpsc::channel();
                    let clean_stack_sink = clean_stack_sink.clone();

                    std::thread::spawn(move || {
                        for _ in 0..10 {
                            if clean_stack_sink.send(Stack::new()).is_err() {
                                return;
                            }
                        }

                        loop {
                            let Ok(mut stack): Result<Box<Stack>, _> = dirty_stack_source.recv()
                            else {
                                return;
                            };
                            stack.zero();
                            if clean_stack_sink.send(stack).is_err() {
                                return;
                            }
                        }
                    });

                    dirty_stack_sink
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

impl StackPool {
    pub fn get(&mut self) -> Box<Stack> {
        self.clean_stack_source.recv().unwrap()
    }

    pub fn recycle(&mut self, stack: Box<Stack>) {
        self.dirty_stack_sinks[self.current_cleaner]
            .send(stack)
            .unwrap();
        self.current_cleaner = (self.current_cleaner + 1) % CLEANERS;
    }
}
