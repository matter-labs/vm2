use primitive_types::{H160, U256};
use zksync_vm2_interface::{HeapId, Tracer};

use crate::{
    addressing_modes::Addressable,
    callframe::{Callframe, CallframeSnapshot},
    fat_pointer::FatPointer,
    heap::Heaps,
    page_ids::{first_dynamic_base_page, next_page_group},
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
    pub(crate) next_base_page: u32,
    /// Whether the current instruction has written its second output (`dst1`) register.
    /// Set by every `dst1` write; if still `false` after execution, `dst1` is cleared to zero,
    /// matching `zk_evm`. Transient per-instruction bookkeeping; excluded from equality and snapshots.
    pub(crate) dst1_was_updated: bool,
}

impl<T, W> State<T, W> {
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
                false,
                false,
                world_before_this_frame,
            ),
            previous_frames: vec![],

            heaps: Heaps::new(calldata),

            transaction_number: 0,
            context_u128: 0,
            next_base_page: first_dynamic_base_page(),
            dst1_was_updated: false,
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

    pub(crate) fn set_context_u128(&mut self, value: u128) {
        self.context_u128 = value;
    }

    pub(crate) fn get_context_u128(&self) -> u128 {
        self.current_frame.context_u128
    }

    pub(crate) const fn next_base_page(&self) -> u32 {
        self.next_base_page
    }

    /// Reserves the next far-call page group and returns its base page.
    ///
    /// The returned base page is used to derive the pages owned by a new frame,
    /// such as its heap and aux heap.
    pub(crate) fn allocate_base_page(&mut self) -> u32 {
        let base_page = self.next_base_page;
        self.next_base_page = next_page_group(self.next_base_page);
        base_page
    }
}

impl<T: Tracer, W: World<T>> State<T, W> {
    /// Returns the total unspent gas in the VM, including stipends.
    pub(crate) fn total_unspent_gas(&self) -> u32 {
        self.current_frame.gas
            + self
                .previous_frames
                .iter()
                .map(Callframe::contained_gas)
                .sum::<u32>()
    }

    pub(crate) fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            registers: self.registers,
            register_pointer_flags: self.register_pointer_flags,
            flags: self.flags.clone(),
            bootloader_frame: self.current_frame.snapshot(),
            bootloader_heap_snapshot: self.heaps.snapshot(),
            dynamic_heap_groups: self.heaps.dynamic_len(),
            transaction_number: self.transaction_number,
            context_u128: self.context_u128,
            next_base_page: self.next_base_page,
        }
    }

    pub(crate) fn rollback(
        &mut self,
        snapshot: StateSnapshot,
        mut is_heap_pinned: impl FnMut(HeapId) -> bool,
    ) {
        let StateSnapshot {
            registers,
            register_pointer_flags,
            flags,
            bootloader_frame,
            bootloader_heap_snapshot,
            dynamic_heap_groups,
            transaction_number,
            context_u128,
            next_base_page,
        } = snapshot;

        for heap in self.current_frame.rollback(bootloader_frame) {
            if !is_heap_pinned(heap) {
                self.heaps.deallocate(heap);
            }
        }
        self.heaps.rollback(bootloader_heap_snapshot);

        // Pages created after the host snapshot may no longer be reachable from any frame-level
        // keep-alive list. Decommit pages are the important case: rollback first removes the
        // global pin, so frame bookkeeping alone can miss a dynamic page that was kept alive only
        // by that pin.
        self.heaps.truncate_dynamic_to(dynamic_heap_groups);

        self.registers = registers;
        self.register_pointer_flags = register_pointer_flags;
        self.flags = flags;
        self.transaction_number = transaction_number;
        self.context_u128 = context_u128;
        self.next_base_page = next_base_page;

        // Transient per-instruction bookkeeping, not part of the snapshot. It is always reset at the
        // start of the next instruction before being read, so this is a functional no-op; we clear
        // it here to keep the invariant "never survives an instruction boundary" self-evident.
        self.dst1_was_updated = false;
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
            next_base_page: self.next_base_page,
            dst1_was_updated: self.dst1_was_updated,
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
            && self.next_base_page == other.next_base_page
            && self.current_frame == other.current_frame
            && self.previous_frames == other.previous_frames
            && self.heaps == other.heaps
    }
}

impl<T, W> Addressable for State<T, W> {
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

    fn mark_dst1_written(&mut self) {
        self.dst1_was_updated = true;
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
    dynamic_heap_groups: usize,
    transaction_number: u16,
    context_u128: u128,
    next_base_page: u32,
}
