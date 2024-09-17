use primitive_types::{H160, U256};

/// Public interface of the VM state. Encompasses both read and write methods.
pub trait StateInterface {
    /// Reads a register with the specified zero-based index. Returns a value together with a pointer flag.
    fn read_register(&self, register: u8) -> (U256, bool);
    /// Sets a register with the specified zero-based index
    fn set_register(&mut self, register: u8, value: U256, is_pointer: bool);

    /// Returns a mutable handle to the current call frame.
    fn current_frame(&mut self) -> impl CallframeInterface + '_;
    /// Returns the total number of call frames.
    fn number_of_callframes(&self) -> usize;
    /// Returns a mutable handle to a call frame with the specified index, where
    /// zero is the current frame, one is the frame before that etc.
    fn callframe(&mut self, n: usize) -> impl CallframeInterface + '_;

    /// Reads a single byte from the specified heap at the specified 0-based offset.
    fn read_heap_byte(&self, heap: HeapId, offset: u32) -> u8;
    /// Reads an entire `U256` word in the big-endian order from the specified heap / `offset`
    /// (which is the index of the most significant byte of the read value).
    fn read_heap_u256(&self, heap: HeapId, offset: u32) -> U256;
    /// Writes an entire `U256` word in the big-endian order to the specified heap at the specified `offset`
    /// (which is the index of the most significant byte of the written value).
    fn write_heap_u256(&mut self, heap: HeapId, offset: u32, value: U256);

    /// Returns current execution flags.
    fn flags(&self) -> Flags;
    /// Sets current execution flags.
    fn set_flags(&mut self, flags: Flags);

    /// Returns the currently set 0-based transaction number.
    fn transaction_number(&self) -> u16;
    /// Sets the current transaction number.
    fn set_transaction_number(&mut self, value: u16);

    /// Returns the value of the context register.
    fn context_u128_register(&self) -> u128;
    /// Sets the value of the context register.
    fn set_context_u128_register(&mut self, value: u128);

    /// Iterates over storage slots read or written during VM execution.
    fn get_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)>;
    /// Iterates over all transient storage slots set during VM execution.
    fn get_transient_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)>;
    /// Gets value of the specified transient storage slot.
    fn get_transient_storage(&self, address: H160, slot: U256) -> U256;
    /// Sets value of the specified transient storage slot.
    fn write_transient_storage(&mut self, address: H160, slot: U256, value: U256);

    /// Iterates over events emitted during VM execution.
    fn events(&self) -> impl Iterator<Item = Event>;
    /// Iterates over L2-to-L1 logs emitted during VM execution.
    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log>;

    /// Gets the current amount of published pubdata.
    fn pubdata(&self) -> i32;
    /// Sets the current amount of published pubdata.
    fn set_pubdata(&mut self, value: i32);
}

/// VM execution flags. See the EraVM reference for more details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Flags {
    /// "Less than" flag.
    pub less_than: bool,
    /// "Equal" flag.
    pub equal: bool,
    /// "Greater than" flag.
    pub greater: bool,
}

/// Public interface of an EraVM call frame.
pub trait CallframeInterface {
    /// Address of the storage context associated with this frame. For delegate calls, this address is inherited from the calling contract;
    /// otherwise, it's the same as [`Self::code_address()`].
    fn address(&self) -> H160;
    /// Sets the address of the executing contract.
    fn set_address(&mut self, address: H160);
    /// Address of the contract being executed.
    fn code_address(&self) -> H160;
    /// Sets the address of the contract being executed. Does not cause the contract at the specified address get loaded per se, just updates
    /// the value used internally by the VM (e.g., returned by the [`CodeAddress`](crate::opcodes::CodeAddress) opcode).
    fn set_code_address(&mut self, address: H160);
    /// Address of the calling contract. Respects delegate and mimic calls.
    fn caller(&self) -> H160;
    /// Sets the address of the calling contract.
    fn set_caller(&mut self, address: H160);

    /// Returns the current program counter (i.e., 0-based index of the instruction being executed).
    /// During panic this returns `None`.
    fn program_counter(&self) -> Option<u16>;
    /// Sets the program counter.
    /// The VM will execute an invalid instruction if you jump out of the program.
    fn set_program_counter(&mut self, value: u16);

    /// Returns the program counter that the parent frame should continue from if this frame fails.
    fn exception_handler(&self) -> u16;
    /// Sets the exception handler as specified [above](Self::exception_handler()).
    fn set_exception_handler(&mut self, value: u16);

    /// Checks whether the call is static.
    fn is_static(&self) -> bool;
    /// Checks whether the call is executed in kernel mode.
    fn is_kernel(&self) -> bool;

    /// Returns the remaining amount of gas.
    fn gas(&self) -> u32;
    /// Sets the remaining amount of gas.
    fn set_gas(&mut self, new_gas: u32);
    /// Additional gas provided for the duration of this callframe.
    fn stipend(&self) -> u32;

    /// Returns the context value for this call. This context is accessible via [`ContextU128`](crate::opcodes::ContextU128) opcode.
    fn context_u128(&self) -> u128;
    /// Sets the context value for this call.
    fn set_context_u128(&mut self, value: u128);

    /// Checks whether this frame corresponds to a near call.
    fn is_near_call(&self) -> bool;

    /// Reads the specified stack slot. Returns a value together with a pointer flag.
    fn read_stack(&self, index: u16) -> (U256, bool);
    /// Sets the value and pointer flag for the specified stack slot.
    fn write_stack(&mut self, index: u16, value: U256, is_pointer: bool);

    /// Returns the stack pointer.
    fn stack_pointer(&self) -> u16;
    /// Sets the stack pointer.
    fn set_stack_pointer(&mut self, value: u16);

    /// Returns ID of the main heap used in this call.
    fn heap(&self) -> HeapId;
    /// Returns the main heap boundary (number of paid bytes).
    fn heap_bound(&self) -> u32;
    /// Sets the main heap boundary.
    fn set_heap_bound(&mut self, value: u32);

    /// Returns ID of the auxiliary heap used in this call.
    fn aux_heap(&self) -> HeapId;
    /// Returns the auxiliary heap boundary (number of paid bytes).
    fn aux_heap_bound(&self) -> u32;
    /// Sets the auxiliary heap boundary.
    fn set_aux_heap_bound(&mut self, value: u32);

    /// Reads a word from the bytecode of the executing contract.
    fn read_contract_code(&self, slot: u16) -> U256;
}

/// Identifier of a VM heap.
///
/// EraVM docs sometimes refer to heaps as *heap pages*; docs in these crate don't to avoid confusion with internal heap structure.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct HeapId(u32);

impl HeapId {
    /// Identifier of the calldata heap used by the first executed program (i.e., the bootloader).
    pub const FIRST_CALLDATA: Self = Self(1);
    /// Identifier of the heap used by the first executed program (i.e., the bootloader).
    pub const FIRST: Self = Self(2);
    /// Identifier of the auxiliary heap used by the first executed program (i.e., the bootloader)
    pub const FIRST_AUX: Self = Self(3);

    /// Only for dealing with external data structures, never use internally.
    #[doc(hidden)]
    pub const fn from_u32_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// Converts this ID to an integer value.
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// Event emitted by EraVM.
///
/// There is no address field because nobody is interested in events that don't come
/// from the event writer, so we simply do not record events coming from anywhere else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Event key.
    pub key: U256,
    /// Event value.
    pub value: U256,
    /// Is this event first in a chain of events?
    pub is_first: bool,
    /// Shard identifier (currently, always set to 0).
    pub shard_id: u8,
    /// 0-based index of a transaction that has emitted this event.
    pub tx_number: u16,
}

/// L2-to-L1 log emitted by EraVM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct L2ToL1Log {
    /// Log key.
    pub key: U256,
    /// Log value.
    pub value: U256,
    /// Is this a service log?
    pub is_service: bool,
    /// Address of the contract that has emitted this log.
    pub address: H160,
    /// Shard identifier (currently, always set to 0).
    pub shard_id: u8,
    /// 0-based index of a transaction that has emitted this event.
    pub tx_number: u16,
}

#[cfg(test)]
#[derive(Debug)]
pub struct DummyState;

#[cfg(test)]
impl StateInterface for DummyState {
    fn read_register(&self, _: u8) -> (U256, bool) {
        unimplemented!()
    }

    fn set_register(&mut self, _: u8, _: U256, _: bool) {
        unimplemented!()
    }

    fn current_frame(&mut self) -> impl CallframeInterface + '_ {
        DummyState
    }

    fn number_of_callframes(&self) -> usize {
        unimplemented!()
    }

    fn callframe(&mut self, _: usize) -> impl CallframeInterface + '_ {
        DummyState
    }

    fn read_heap_byte(&self, _: HeapId, _: u32) -> u8 {
        unimplemented!()
    }

    fn read_heap_u256(&self, _: HeapId, _: u32) -> U256 {
        unimplemented!()
    }

    fn write_heap_u256(&mut self, _: HeapId, _: u32, _: U256) {
        unimplemented!()
    }

    fn flags(&self) -> Flags {
        unimplemented!()
    }

    fn set_flags(&mut self, _: Flags) {
        unimplemented!()
    }

    fn transaction_number(&self) -> u16 {
        unimplemented!()
    }

    fn set_transaction_number(&mut self, _: u16) {
        unimplemented!()
    }

    fn context_u128_register(&self) -> u128 {
        unimplemented!()
    }

    fn set_context_u128_register(&mut self, _: u128) {
        unimplemented!()
    }

    fn get_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        std::iter::empty()
    }

    fn get_transient_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        std::iter::empty()
    }

    fn get_transient_storage(&self, _: H160, _: U256) -> U256 {
        unimplemented!()
    }

    fn write_transient_storage(&mut self, _: H160, _: U256, _: U256) {
        unimplemented!()
    }

    fn events(&self) -> impl Iterator<Item = Event> {
        std::iter::empty()
    }

    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log> {
        std::iter::empty()
    }

    fn pubdata(&self) -> i32 {
        unimplemented!()
    }

    fn set_pubdata(&mut self, _: i32) {
        unimplemented!()
    }
}

#[cfg(test)]
impl CallframeInterface for DummyState {
    fn address(&self) -> H160 {
        unimplemented!()
    }

    fn set_address(&mut self, _: H160) {
        unimplemented!()
    }

    fn code_address(&self) -> H160 {
        unimplemented!()
    }

    fn set_code_address(&mut self, _: H160) {
        unimplemented!()
    }

    fn caller(&self) -> H160 {
        unimplemented!()
    }

    fn set_caller(&mut self, _: H160) {
        unimplemented!()
    }

    fn program_counter(&self) -> Option<u16> {
        unimplemented!()
    }

    fn set_program_counter(&mut self, _: u16) {
        unimplemented!()
    }

    fn exception_handler(&self) -> u16 {
        unimplemented!()
    }

    fn set_exception_handler(&mut self, _: u16) {
        unimplemented!()
    }

    fn is_static(&self) -> bool {
        unimplemented!()
    }

    fn is_kernel(&self) -> bool {
        unimplemented!()
    }

    fn gas(&self) -> u32 {
        unimplemented!()
    }

    fn set_gas(&mut self, _: u32) {
        unimplemented!()
    }

    fn stipend(&self) -> u32 {
        unimplemented!()
    }

    fn context_u128(&self) -> u128 {
        unimplemented!()
    }

    fn set_context_u128(&mut self, _: u128) {
        unimplemented!()
    }

    fn is_near_call(&self) -> bool {
        unimplemented!()
    }

    fn read_stack(&self, _: u16) -> (U256, bool) {
        unimplemented!()
    }

    fn write_stack(&mut self, _: u16, _: U256, _: bool) {
        unimplemented!()
    }

    fn stack_pointer(&self) -> u16 {
        unimplemented!()
    }

    fn set_stack_pointer(&mut self, _: u16) {
        unimplemented!()
    }

    fn heap(&self) -> HeapId {
        unimplemented!()
    }

    fn heap_bound(&self) -> u32 {
        unimplemented!()
    }

    fn set_heap_bound(&mut self, _: u32) {
        unimplemented!()
    }

    fn aux_heap(&self) -> HeapId {
        unimplemented!()
    }

    fn aux_heap_bound(&self) -> u32 {
        unimplemented!()
    }

    fn set_aux_heap_bound(&mut self, _: u32) {
        unimplemented!()
    }

    fn read_contract_code(&self, _: u16) -> U256 {
        unimplemented!()
    }
}
