use crate::addressing_modes::Arguments;
use crate::instruction::ExecutionStatus;
use crate::instruction_handlers::RETURN_COST;
use crate::state::StateSnapshot;
use crate::world_diff::ExternalSnapshot;
use crate::{
    callframe::{Callframe, FrameRemnant},
    decommit::u256_into_address,
    stack::StackPool,
    state::State,
    world_diff::{Snapshot, WorldDiff},
    ExecutionEnd, Program,
};
use crate::{Instruction, ModeRequirements, Predicate};
use eravm_stable_interface::opcodes::TypeLevelCallingMode;
use eravm_stable_interface::{CallingMode, HeapId, Tracer};
use u256::H160;

pub(crate) const STORAGE_READ_STORAGE_APPLICATION_CYCLES: u32 = 1;
pub(crate) const STORAGE_WRITE_STORAGE_APPLICATION_CYCLES: u32 = 2;

#[derive(Debug)]
pub struct Settings {
    pub default_aa_code_hash: [u8; 32],
    pub evm_interpreter_code_hash: [u8; 32],

    /// Writing to this address in the bootloader's heap suspends execution
    pub hook_address: u32,
}

pub struct VirtualMachine<T, W> {
    pub world_diff: WorldDiff,

    /// Storing the state in a separate struct is not just cosmetic.
    /// The state couldn't be passed to the world if it was inlined.
    pub state: State<T, W>,

    pub(crate) settings: Settings,

    pub(crate) stack_pool: StackPool,

    /// Instruction that is jumped to when things go wrong while executing another.
    /// Boxed, so the pointer isn't invalidated by moves.
    pub(crate) panic: Box<Instruction<T, W>>,
}

impl<T: Tracer, W> VirtualMachine<T, W> {
    pub fn new(
        address: H160,
        program: Program<T, W>,
        caller: H160,
        calldata: Vec<u8>,
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
                &calldata,
                gas,
                program,
                world_before_this_frame,
                stack_pool.get(),
            ),
            settings,
            stack_pool,
            panic: Box::new(Instruction::from_panic(
                None,
                Arguments::new(Predicate::Always, RETURN_COST, ModeRequirements::none()),
            )),
        }
    }

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

    /// Returns a compact representation of the VM's current state,
    /// including pending side effects like storage changes and emitted events.
    /// [VirtualMachine::rollback] can be used to return the VM to this state.
    /// # Panics
    /// Calling this function outside of the initial callframe is not allowed.
    pub fn snapshot(&self) -> VmSnapshot {
        assert!(
            self.state.previous_frames.is_empty(),
            "Snapshotting is only allowed in the bootloader!"
        );
        VmSnapshot {
            world_snapshot: self.world_diff.external_snapshot(),
            state_snapshot: self.state.snapshot(),
        }
    }

    /// Returns the VM to the state it was in when the snapshot was created.
    /// # Panics
    /// Rolling back snapshots in anything but LIFO order may panic.
    /// Rolling back outside the initial callframe will panic.
    pub fn rollback(&mut self, snapshot: VmSnapshot) {
        assert!(
            self.state.previous_frames.is_empty(),
            "Rolling back is only allowed in the bootloader!"
        );
        self.world_diff.external_rollback(snapshot.world_snapshot);
        self.state.rollback(snapshot.state_snapshot);
    }

    /// This must only be called when it is known that the VM cannot be rolled back,
    /// so there must not be any external snapshots and the callstack
    /// should ideally be empty, though in practice it sometimes contains
    /// a near call inside the bootloader.
    pub fn delete_history(&mut self) {
        self.world_diff.delete_history();
        self.state.delete_history();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_frame<M: TypeLevelCallingMode>(
        &mut self,
        code_address: H160,
        program: Program<T, W>,
        gas: u32,
        stipend: u32,
        exception_handler: u16,
        is_static: bool,
        calldata_heap: HeapId,
        world_before_this_frame: Snapshot,
    ) {
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
            self.state.heaps.allocate(),
            self.state.heaps.allocate(),
            calldata_heap,
            gas,
            stipend,
            exception_handler,
            if M::VALUE == CallingMode::Delegate {
                self.state.current_frame.context_u128
            } else {
                self.state.context_u128
            },
            is_static,
            world_before_this_frame,
        );
        self.state.context_u128 = 0;

        std::mem::swap(&mut new_frame, &mut self.state.current_frame);
        self.state.previous_frames.push(new_frame);
    }

    pub(crate) fn pop_frame(&mut self, heap_to_keep: Option<HeapId>) -> Option<FrameRemnant> {
        self.state.previous_frames.pop().map(|mut frame| {
            for &heap in [
                self.state.current_frame.heap,
                self.state.current_frame.aux_heap,
            ]
            .iter()
            .chain(&self.state.current_frame.heaps_i_am_keeping_alive)
            {
                if Some(heap) != heap_to_keep {
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

            FrameRemnant {
                exception_handler,
                snapshot: world_before_this_frame,
            }
        })
    }

    pub(crate) fn start_new_tx(&mut self) {
        self.state.transaction_number = self.state.transaction_number.wrapping_add(1);
        self.world_diff.clear_transient_storage()
    }
}

#[derive(Debug)]
pub struct VmSnapshot {
    world_snapshot: ExternalSnapshot,
    state_snapshot: StateSnapshot,
}
