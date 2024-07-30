use u256::{H160, U256};

pub trait StateInterface {
    fn read_register(&self, register: u8) -> (U256, bool);
    fn set_register(&mut self, register: u8, value: U256, is_pointer: bool);

    fn number_of_callframes(&self) -> usize;
    /// zero is the current frame, one is the frame before that etc.
    fn callframe(&mut self, n: usize) -> &mut impl CallframeInterface;

    fn read_heap_byte(&self, heap: HeapId, index: u32) -> u8;
    fn write_heap_byte(&mut self, heap: HeapId, index: u32, byte: u8);

    fn flags(&self) -> Flags;
    fn set_flags(&mut self, flags: Flags);

    fn transaction_number(&self) -> u16;
    fn set_transaction_number(&mut self, value: u16);

    fn context_u128_register(&self) -> u128;
    fn set_context_u128_register(&mut self, value: u128);

    /// Returns the new value and the amount of pubdata paid for it.
    fn get_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)>;
    fn get_storage(&self, address: H160, slot: U256) -> Option<(U256, u32)>;
    fn get_storage_initial_value(&self, address: H160, slot: U256) -> U256;
    fn write_storage(&mut self, address: H160, slot: U256, value: U256);

    fn get_transient_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)>;
    fn get_transient_storage(&self, address: H160, slot: U256) -> U256;
    fn write_transient_storage(&mut self, address: H160, slot: U256, value: U256);

    fn events(&self) -> impl Iterator<Item = Event>;
    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log>;

    fn pubdata(&self) -> i32;
    fn set_pubdata(&mut self, value: i32);

    /// it is run in kernel mode
    fn run_arbitrary_code(code: &[u64]);

    fn static_heap(&self) -> HeapId;
}

pub struct Flags {
    pub less_than: bool,
    pub equal: bool,
    pub greater: bool,
}

pub trait CallframeInterface {
    fn address(&self) -> H160;
    fn set_address(&mut self, address: H160);
    fn code_address(&self) -> H160;
    fn set_code_address(&mut self, address: H160);
    fn caller(&self) -> H160;
    fn set_caller(&mut self, address: H160);

    /// During panic and arbitrary code execution this returns None.
    fn program_counter(&self) -> Option<u16>;

    /// The VM will execute an invalid instruction if you jump out of the program.
    fn set_program_counter(&mut self, value: u16);

    fn exception_handler(&self) -> u16;

    fn is_static(&self) -> bool;
    fn is_kernel(&self) -> bool {
        todo!()
    }

    fn gas(&self) -> u32;
    fn set_gas(&mut self, new_gas: u32);
    fn stipend(&self) -> u32;

    fn context_u128(&self) -> u128;
    fn set_context_u128(&mut self, value: u128);

    fn is_near_call(&self) -> bool;

    fn read_stack(&self, register: u16) -> (U256, bool);
    fn write_stack(&mut self, register: u16, value: U256, is_pointer: bool);

    fn stack_pointer(&self) -> u16;
    fn set_stack_pointer(&mut self, value: u16);

    fn heap(&self) -> HeapId;
    fn heap_bound(&self) -> u32;
    fn set_heap_bound(&mut self, value: u32);

    fn aux_heap(&self) -> HeapId;
    fn aux_heap_bound(&self) -> u32;
    fn set_aux_heap_bound(&mut self, value: u32);

    fn read_code_page(&self, slot: u16) -> U256;
}

pub trait Tracer {
    fn before_instruction<S: StateInterface>(&mut self, state: &mut S);
    fn after_instruction<S: StateInterface>(&mut self, state: &mut S);
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct HeapId(u32);

impl HeapId {
    /// Only for dealing with external data structures, never use internally.
    pub const fn from_u32_unchecked(value: u32) -> Self {
        Self(value)
    }

    pub const fn to_u32(self) -> u32 {
        self.0
    }
}

/// There is no address field because nobody is interested in events that don't come
/// from the event writer, so we simply do not record events coming frome anywhere else.
#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u16,
}

#[derive(Debug)]
pub struct L2ToL1Log {
    pub key: U256,
    pub value: U256,
    pub is_service: bool,
    pub address: H160,
    pub shard_id: u8,
    pub tx_number: u16,
}
