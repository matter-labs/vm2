use crate::{
    addressing_modes::Addressable,
    callframe::{Callframe, CallframeSnapshot},
    fat_pointer::FatPointer,
    heap::{Heaps, CALLDATA_HEAP, FIRST_AUX_HEAP, FIRST_HEAP},
    predication::Flags,
    program::Program,
    stack::Stack,
    world_diff::Snapshot,
};
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
            memory_page: CALLDATA_HEAP,
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
                FIRST_AUX_HEAP,
                CALLDATA_HEAP,
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
                .map(|(_, frame)| frame.contained_gas())
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
    fn stack_pointer(&mut self) -> &mut u16 {
        &mut self.current_frame.sp
    }

    fn read_stack_pointer_flag(&mut self, slot: u16) -> bool {
        self.current_frame.stack.get_pointer_flag(slot)
    }
    fn set_stack_pointer_flag(&mut self, slot: u16) {
        self.current_frame.stack.set_pointer_flag(slot)
    }
    fn clear_stack_pointer_flag(&mut self, slot: u16) {
        self.current_frame.stack.clear_pointer_flag(slot)
    }

    fn code_page(&self) -> &[U256] {
        self.current_frame.program.code_page()
    }

    fn in_kernel_mode(&self) -> bool {
        self.current_frame.is_kernel
    }
}

pub(crate) struct StateSnapshot {
    registers: [U256; 16],
    register_pointer_flags: u16,

    flags: Flags,

    bootloader_frame: CallframeSnapshot,

    bootloader_heap_snapshot: (usize, usize),
    transaction_number: u16,

    context_u128: u128,
}
