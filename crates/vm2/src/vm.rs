use std::fmt;

use primitive_types::{H160, U256};
use zksync_vm2_interface::{opcodes::TypeLevelCallingMode, CallingMode, HeapId, Tracer};

use crate::{
    callframe::{Callframe, FrameRemnant},
    decommit::{materialize_decommit_page, u256_into_address},
    instruction::ExecutionStatus,
    page_ids::{aux_heap_page_from_base, code_page_from_base, heap_page_from_base},
    stack::StackPool,
    state::{State, StateSnapshot},
    world_diff::{ExternalSnapshot, Snapshot, WorldDiff},
    ExecutionEnd, Program, World,
};

/// [`VirtualMachine`] settings.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Bytecode hash of the default account abstraction contract.
    pub default_aa_code_hash: [u8; 32],
    /// Bytecode hash of the EVM interpreter.
    pub evm_interpreter_code_hash: [u8; 32],
    /// Writing to this address in the bootloader's heap suspends execution
    pub hook_address: u32,
}

/// High-performance out-of-circuit EraVM implementation.
#[derive(Debug)]
pub struct VirtualMachine<T, W> {
    pub(crate) world_diff: WorldDiff,
    pub(crate) state: State<T, W>,
    pub(crate) settings: Settings,
    pub(crate) stack_pool: StackPool,
    pub(crate) snapshot: Option<VmSnapshot>,
}

impl<T: Tracer, W: World<T>> VirtualMachine<T, W> {
    /// Creates a new VM instance.
    pub fn new(
        address: H160,
        program: Program<T, W>,
        caller: H160,
        calldata: &[u8],
        gas: u32,
        settings: Settings,
    ) -> Self {
        let world_diff = WorldDiff::default();
        let world_before_this_frame = world_diff.snapshot();
        let mut stack_pool = StackPool::default();

        Self {
            world_diff,
            state: State::new(
                address,
                caller,
                calldata,
                gas,
                program,
                world_before_this_frame,
                stack_pool.get(),
            ),
            settings,
            stack_pool,
            snapshot: None,
        }
    }

    /// Pre-reserve dynamic heap-group capacity to suppress mid-execution
    /// Vec doublings inside `Heaps`. Hint with the worst-case far-call
    /// count estimable from the witness.
    pub fn reserve_dynamic_heap_capacity(&mut self, n: usize) {
        self.state.heaps.reserve_dynamic_groups(n);
    }

    /// Provides a reference to the [`World`] diff accumulated by VM execution so far.
    pub fn world_diff(&self) -> &WorldDiff {
        &self.world_diff
    }

    /// Provides a mutable reference to the [`World`] diff accumulated by VM execution so far.
    ///
    /// It is unsound to mutate [`WorldDiff`] in the middle of VM execution in the general case; thus, this method should only be used in tests.
    #[doc(hidden)]
    pub fn world_diff_mut(&mut self) -> &mut WorldDiff {
        &mut self.world_diff
    }

    /// Manually warms up a decommit of the code with the provided `code_hash`, materializing its
    /// heap page exactly like the [`Decommit`](zksync_vm2_interface::opcodes::Decommit) opcode
    /// handler does. Returns `true` if this was a fresh decommit (i.e., the code wasn't decommitted
    /// previously in the same VM run).
    ///
    /// Unlike [`WorldDiff::decommit_opcode`], this records the decommit in the VM state (assigning a
    /// reusable page), so a subsequent `decommit` opcode on the same hash is recognized as
    /// already-decommitted and refunded — matching legacy `zk_evm` `execute_decommit` semantics.
    ///
    /// This is intended for execution-verification setup / tests; it can break VM operation if
    /// called in the middle of execution.
    #[doc(hidden)]
    pub fn manually_decommit(&mut self, world: &mut W, tracer: &mut T, code_hash: U256) -> bool {
        let (code, is_fresh) = self.world_diff.decommit_opcode(world, tracer, code_hash);
        if is_fresh {
            // Materialize into a fresh code page rather than reusing the current frame's heap, so
            // manual decommits performed between frames (e.g. against the bootloader frame) don't
            // clobber live heap data.
            let base_page = self.state.allocate_base_page();
            materialize_decommit_page(self, code_hash, &code, code_page_from_base(base_page));
        }
        is_fresh
    }

    /// Runs this VM with the specified [`World`] and [`Tracer`] until an end of execution due to a hook, or an error.
    pub fn run(&mut self, world: &mut W, tracer: &mut T) -> ExecutionEnd {
        unsafe {
            loop {
                if let ExecutionStatus::Stopped(end) =
                    ((*self.state.current_frame.pc).handler)(self, world, tracer)
                {
                    return end;
                }
            }
        }
    }

    /// Returns how much of the extra gas limit is left and the stop reason,
    /// unless the extra gas limit was exceeded.
    ///
    /// Needed to support account validation gas limit.
    /// We cannot simply reduce the available gas, as contracts might behave differently
    /// depending on remaining gas.
    pub fn resume_with_additional_gas_limit(
        &mut self,
        world: &mut W,
        tracer: &mut T,
        gas_limit: u32,
    ) -> Option<(u32, ExecutionEnd)> {
        let minimum_gas = self.state.total_unspent_gas().saturating_sub(gas_limit);

        let end = unsafe {
            loop {
                if let ExecutionStatus::Stopped(end) =
                    ((*self.state.current_frame.pc).handler)(self, world, tracer)
                {
                    break end;
                }

                if self.state.total_unspent_gas() < minimum_gas {
                    return None;
                }
            }
        };

        self.state
            .total_unspent_gas()
            .checked_sub(minimum_gas)
            .map(|left| (left, end))
    }

    /// Creates a VM snapshot. The snapshot can then be rolled back to, or discarded.
    ///
    /// # Panics
    ///
    /// - Panics if called outside the initial (bootloader) callframe.
    /// - Panics if this VM already has a snapshot.
    pub fn make_snapshot(&mut self) {
        assert!(self.snapshot.is_none(), "VM already has a snapshot");
        assert!(
            self.state.previous_frames.is_empty(),
            "Snapshotting is only allowed in the bootloader"
        );

        self.snapshot = Some(VmSnapshot {
            world_snapshot: self.world_diff.external_snapshot(),
            state_snapshot: self.state.snapshot(),
        });
    }

    /// Returns the VM to the state it was in when [`Self::make_snapshot()`] was called.
    ///
    /// # Panics
    ///
    /// - Panics if this VM doesn't hold a snapshot.
    /// - Panics if called outside the initial (bootloader) callframe.
    pub fn rollback(&mut self) {
        assert!(
            self.state.previous_frames.is_empty(),
            "Rolling back is only allowed in the bootloader"
        );

        let snapshot = self
            .snapshot
            .take()
            .expect("`rollback()` called without a snapshot");
        self.world_diff.external_rollback(snapshot.world_snapshot);
        self.state.rollback(snapshot.state_snapshot, |heap| {
            self.world_diff.is_decommit_page_pinned(heap)
        });
        self.delete_history();
    }

    /// Pops a [previously made](Self::make_snapshot()) snapshot without rolling back to it. This effectively commits
    /// all changes made up to this point, so that they cannot be rolled back.
    ///
    /// # Panics
    ///
    /// - Panics if called outside the initial (bootloader) callframe.
    pub fn pop_snapshot(&mut self) {
        assert!(
            self.state.previous_frames.is_empty(),
            "Popping a snapshot is only allowed in the bootloader"
        );
        self.snapshot = None;
        self.delete_history();
        self.reclaim_bootloader_returndata_heaps();
    }

    /// Frees the returndata heaps that accumulated on the bootloader frame while
    /// the just-committed transaction(s) executed.
    ///
    /// Every far-call keeps the heap holding its returndata alive and bubbles it
    /// up to the caller (see [`Self::pop_frame`]); heaps forwarded all the way to
    /// the bootloader frame — which never pops during a batch — otherwise live
    /// until the VM is dropped, so they accumulate across every transaction (the
    /// dominant heap-memory consumer on large batches).
    ///
    /// This is the safe point to release them: the callstack is unwound to the
    /// bootloader (`previous_frames` is empty), the external snapshot has just
    /// been discarded (`self.snapshot` is `None`) and history deleted, so no
    /// rollback can reference these heaps; and a committed transaction's
    /// returndata is dead once the bootloader moves on. Decommit-pinned code
    /// pages (shared across transactions by hash) are kept — the same predicate
    /// [`Self::pop_frame`] uses. Freed pages return to the `PagePool` and are
    /// reused by the next transaction, so peak page usage stays at roughly one
    /// transaction's worth instead of growing with the transaction count.
    fn reclaim_bootloader_returndata_heaps(&mut self) {
        // `kept` is owned after the take, so the `retain` closure can borrow
        // `self.world_diff`/`self.state.heaps` without conflicting with the
        // borrow of the field it compacts. Retain drops the deallocated heaps
        // in place, avoiding a second allocation. Reordering is irrelevant here:
        // there is no live snapshot to consume the tail ordering (see the len()
        // snapshot / tail-drain rollback path in `Callframe`).
        let mut kept = std::mem::take(&mut self.state.current_frame.heaps_i_am_keeping_alive);
        kept.retain(|&heap| {
            let pinned = self.world_diff.is_decommit_page_pinned(heap);
            if !pinned {
                self.state.heaps.deallocate(heap);
            }
            pinned
        });
        self.state.current_frame.heaps_i_am_keeping_alive = kept;
    }

    /// This must only be called when it is known that the VM cannot be rolled back,
    /// so there must not be any external snapshots and the callstack
    /// should ideally be empty, though in practice it sometimes contains
    /// a near call inside the bootloader.
    fn delete_history(&mut self) {
        self.world_diff.delete_history();
        self.state.delete_history();
    }
}

impl<T: Tracer, W> VirtualMachine<T, W> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_frame<M: TypeLevelCallingMode>(
        &mut self,
        code_address: H160,
        program: Program<T, W>,
        gas: u32,
        exception_handler: u16,
        is_static: bool,
        is_evm_blob_format: bool,
        calldata_heap: HeapId,
        world_before_this_frame: Snapshot,
    ) {
        let base_page = self.state.allocate_base_page();
        let heap_page = heap_page_from_base(base_page);
        let aux_heap_page = aux_heap_page_from_base(base_page);
        self.state.heaps.allocate_at(heap_page);
        self.state.heaps.allocate_at(aux_heap_page);

        let mut new_frame = Callframe::new(
            if M::VALUE == CallingMode::Delegate {
                self.state.current_frame.address
            } else {
                code_address
            },
            code_address,
            match M::VALUE {
                CallingMode::Normal => self.state.current_frame.address,
                CallingMode::Delegate => self.state.current_frame.caller,
                CallingMode::Mimic => u256_into_address(self.state.registers[15]),
            },
            program,
            self.stack_pool.get(),
            heap_page,
            aux_heap_page,
            calldata_heap,
            gas,
            exception_handler,
            if M::VALUE == CallingMode::Delegate {
                self.state.current_frame.context_u128
            } else {
                self.state.context_u128
            },
            is_static,
            is_evm_blob_format,
            world_before_this_frame,
        );
        self.state.context_u128 = 0;

        // Count this frame's stipend against the per-user-tx heap-bytes ceiling, but only for
        // non-kernel frames: the bootloader/system contracts get huge free heaps by design and
        // must not inflate a counter that exists to bound *user* memory usage (see
        // `Heaps::live_logical_bytes`). `pop_frame` needs no matching special-case: its existing
        // `deallocate` calls already decrement whatever was counted here (kernel frames were
        // never counted, so deallocating them is a no-op against the counter), and heaps kept
        // alive as returndata are, by construction, not deallocated — so they stay counted, which
        // is exactly the desired accounting.
        if !new_frame.is_kernel {
            self.state
                .heaps
                .set_counted_size(heap_page, new_frame.heap_size);
            self.state
                .heaps
                .set_counted_size(aux_heap_page, new_frame.aux_heap_size);
        }

        std::mem::swap(&mut new_frame, &mut self.state.current_frame);
        self.state.previous_frames.push(new_frame);
    }

    pub(crate) fn pop_frame(&mut self, heap_to_keep: Option<HeapId>) -> Option<FrameRemnant> {
        let mut frame = self.state.previous_frames.pop()?;

        for &heap in [
            self.state.current_frame.heap,
            self.state.current_frame.aux_heap,
        ]
        .iter()
        .chain(&self.state.current_frame.heaps_i_am_keeping_alive)
        {
            if Some(heap) != heap_to_keep && !self.world_diff.is_decommit_page_pinned(heap) {
                self.state.heaps.deallocate(heap);
            }
        }

        std::mem::swap(&mut self.state.current_frame, &mut frame);
        let Callframe {
            exception_handler,
            world_before_this_frame,
            stack,
            ..
        } = frame;

        self.stack_pool.recycle(stack);

        self.state
            .current_frame
            .heaps_i_am_keeping_alive
            .extend(heap_to_keep);

        Some(FrameRemnant {
            exception_handler,
            snapshot: world_before_this_frame,
        })
    }

    pub(crate) fn start_new_tx(&mut self) {
        self.state.transaction_number = self.state.transaction_number.wrapping_add(1);
        self.world_diff.clear_transient_storage();
    }
}

impl<T: fmt::Debug, W: fmt::Debug> VirtualMachine<T, W> {
    /// Dumps an opaque representation of the current VM state.
    #[doc(hidden)] // should only be used in tests
    pub fn dump_state(&self) -> impl PartialEq + fmt::Debug {
        self.state.clone()
    }
}

/// Snapshot of a [`VirtualMachine`].
#[derive(Debug)]
pub(crate) struct VmSnapshot {
    world_snapshot: ExternalSnapshot,
    state_snapshot: StateSnapshot,
}
