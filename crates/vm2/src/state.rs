use primitive_types::{H160, U256};
use zksync_vm2_interface::{HeapId, Tracer};

use crate::{
    addressing_modes::Addressable,
    callframe::{Callframe, CallframeSnapshot},
    fat_pointer::FatPointer,
    heap::Heaps,
    instruction_handlers::spontaneous_panic,
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
    /// Set when an invariant violation requires unwinding the whole transaction.
    /// Transient: consumed by the unwind in `naked_ret`; excluded from equality and snapshots.
    pub(crate) aborting: bool,
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
            aborting: false,
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
        // Order matters for `live_logical_bytes`: `rollback` flat-overwrites the counter to its
        // snapshotted value, and dynamic heap groups only ever grow within a snapshot's scope, so
        // that overwrite already accounts for everything `truncate_dynamic_to` is about to drop.
        // `truncate_dynamic_to` itself does not touch the counter (see its doc comment) — it
        // relies on `rollback` having run first. Do not reorder these two calls.
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
        // Not part of the snapshot either. A rollback undoes a frame's effects via its exception
        // handler, which an active abort always bypasses (see `abort_transaction`'s doc comment),
        // so this can't fire mid-unwind; cleared here for the same self-evidence reason as above.
        self.aborting = false;
    }

    pub(crate) fn delete_history(&mut self) {
        self.heaps.delete_history();
    }

    /// Begin an uncatchable unwind of the entire current transaction.
    /// Callable from inside any instruction handler. The unwind itself runs in
    /// `naked_ret` on subsequent steps, skipping every frame's exception handler
    /// until control returns to the bootloader (frame 0).
    pub(crate) fn abort_transaction(&mut self) {
        self.aborting = true;
        self.current_frame.gas = 0;
        self.current_frame.pc = spontaneous_panic();
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
            aborting: self.aborting,
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
    bootloader_heap_snapshot: (usize, usize, u64),
    dynamic_heap_groups: usize,
    transaction_number: u16,
    context_u128: u128,
    next_base_page: u32,
}

#[cfg(test)]
mod tests {
    use zkevm_opcode_defs::ethereum_types::Address;

    use crate::{
        addressing_modes::{Arguments, Register, Register1},
        instruction_handlers::spontaneous_panic,
        testonly::{initial_decommit, TestWorld},
        Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
    };

    #[test]
    fn abort_transaction_arms_panic_and_burns_gas() {
        let address = Address::from_low_u64_be(0x_1234_5678_90ab_cdef);
        let instructions = vec![Instruction::from_ret(
            Register1(Register::new(0)),
            None,
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        )];
        let mut world = TestWorld::<()>::new(&[(address, Program::from_raw(instructions, vec![]))]);
        let program = initial_decommit(&mut world, address);

        let mut vm = VirtualMachine::new(
            address,
            program,
            Address::zero(),
            &[],
            1000,
            Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        let state = &mut vm.state;
        state.current_frame.gas = 5000;
        assert!(!state.aborting);
        state.abort_transaction();
        assert!(state.aborting);
        assert_eq!(state.current_frame.gas, 0);
        assert_eq!(state.current_frame.pc, spontaneous_panic::<(), _>());
    }
}
