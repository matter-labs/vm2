use primitive_types::{H160, U256};
use zksync_vm2_interface::{HeapId, Tracer};

use crate::{
    addressing_modes::Addressable,
    callframe::{Callframe, CallframeSnapshot},
    fat_pointer::FatPointer,
    heap::Heaps,
    predication::Flags,
    program::Program,
    stack::Stack,
    world_diff::Snapshot,
    World,
};

/// State of a [`VirtualMachine`](crate::VirtualMachine).
#[derive(Debug)]
pub(crate) struct State<T, W> {
    pub(crate) registers: [U256; 16],
    pub(crate) register_pointer_flags: u16,
    pub(crate) flags: Flags,
    pub(crate) current_frame: Callframe<T, W>,
    /// Contains indices to the far call instructions currently being executed.
    /// They are needed to continue execution from the correct spot upon return.
    pub(crate) previous_frames: Vec<Callframe<T, W>>,
    pub(crate) heaps: Heaps,
    pub(crate) transaction_number: u16,
    pub(crate) context_u128: u128,
}

impl<T: Tracer, W: World<T>> State<T, W> {
    pub(crate) fn new(
        address: H160,
        caller: H160,
        calldata: &[u8],
        gas: u32,
        program: Program<T, W>,
        world_before_this_frame: Snapshot,
        stack: Box<Stack>,
    ) -> Self {
        let mut registers: [U256; 16] = Default::default();
        registers[1] = FatPointer {
            memory_page: HeapId::FIRST_CALLDATA,
            offset: 0,
            start: 0,
            length: u32::try_from(calldata.len()).expect("calldata length overflow"),
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
                HeapId::FIRST,
                HeapId::FIRST_AUX,
                HeapId::FIRST_CALLDATA,
                gas,
                0,
                0,
                0,
                false,
                world_before_this_frame,
            ),
            previous_frames: vec![],

            heaps: Heaps::new(calldata),

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
                .map(Callframe::contained_gas)
                .sum::<u32>()
    }

    pub(crate) fn set_context_u128(&mut self, value: u128) {
        self.context_u128 = value;
    }

    pub(crate) fn get_context_u128(&self) -> u128 {
        self.current_frame.context_u128
    }

    pub(crate) fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            registers: self.registers,
            register_pointer_flags: self.register_pointer_flags,
            flags: self.flags.clone(),
            bootloader_frame: self.current_frame.snapshot(),
            bootloader_heap_snapshot: self.heaps.snapshot(),
            transaction_number: self.transaction_number,
            context_u128: self.context_u128,
        }
    }

    pub(crate) fn rollback(&mut self, snapshot: StateSnapshot) {
        let StateSnapshot {
            registers,
            register_pointer_flags,
            flags,
            bootloader_frame,
            bootloader_heap_snapshot,
            transaction_number,
            context_u128,
        } = snapshot;

        for heap in self.current_frame.rollback(bootloader_frame) {
            self.heaps.deallocate(heap);
        }
        self.heaps.rollback(bootloader_heap_snapshot);
        self.registers = registers;
        self.register_pointer_flags = register_pointer_flags;
        self.flags = flags;
        self.transaction_number = transaction_number;
        self.context_u128 = context_u128;
    }

    pub(crate) fn delete_history(&mut self) {
        self.heaps.delete_history();
    }
}

impl<T, W> Clone for State<T, W> {
    fn clone(&self) -> Self {
        Self {
            registers: self.registers,
            register_pointer_flags: self.register_pointer_flags,
            flags: self.flags.clone(),
            current_frame: self.current_frame.clone(),
            previous_frames: self.previous_frames.clone(),
            heaps: self.heaps.clone(),
            transaction_number: self.transaction_number,
            context_u128: self.context_u128,
        }
    }
}

impl<T, W> PartialEq for State<T, W> {
    fn eq(&self, other: &Self) -> bool {
        // does not compare cycle counts to work with tests that
        // expect no change after a rollback
        self.registers == other.registers
            && self.register_pointer_flags == other.register_pointer_flags
            && self.flags == other.flags
            && self.transaction_number == other.transaction_number
            && self.context_u128 == other.context_u128
            && self.current_frame == other.current_frame
            && self.previous_frames == other.previous_frames
            && self.heaps == other.heaps
    }
}

impl<T: Tracer, W: World<T>> Addressable for State<T, W> {
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
        self.current_frame.stack.set(slot, value);
    }

    fn stack_pointer(&mut self) -> &mut u16 {
        &mut self.current_frame.sp
    }

    fn read_stack_pointer_flag(&mut self, slot: u16) -> bool {
        self.current_frame.stack.get_pointer_flag(slot)
    }

    fn set_stack_pointer_flag(&mut self, slot: u16) {
        self.current_frame.stack.set_pointer_flag(slot);
    }

    fn clear_stack_pointer_flag(&mut self, slot: u16) {
        self.current_frame.stack.clear_pointer_flag(slot);
    }

    fn code_page(&self) -> &[U256] {
        self.current_frame.program.code_page()
    }

    fn in_kernel_mode(&self) -> bool {
        self.current_frame.is_kernel
    }
}

#[derive(Debug)]
pub(crate) struct StateSnapshot {
    registers: [U256; 16],
    register_pointer_flags: u16,
    flags: Flags,
    bootloader_frame: CallframeSnapshot,
    bootloader_heap_snapshot: (usize, usize),
    transaction_number: u16,
    context_u128: u128,
}
