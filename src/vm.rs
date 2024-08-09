use crate::heap::HeapId;
use crate::state::StateSnapshot;
use crate::world_diff::ExternalSnapshot;
use crate::{
    callframe::{Callframe, FrameRemnant},
    decommit::u256_into_address,
    instruction_handlers::{free_panic, CallingMode},
    stack::StackPool,
    state::State,
    world_diff::{Snapshot, WorldDiff},
    ExecutionEnd, Instruction, Program, World,
};
use u256::H160;
use zkevm_opcode_defs::{LogOpcode, Opcode, UMAOpcode};

// "Rich addressing" opcodes are opcodes that can write their return value/read the input onto the stack
// and so take 1-2 RAM permutations more than an average opcode.
// In the worst case, a rich addressing may take 3 ram permutations
// (1 for reading the opcode, 1 for writing input value, 1 for writing output value).
pub(crate) const RICH_ADDRESSING_OPCODE_RAM_CYCLES: u32 = 3;

pub(crate) const AVERAGE_OPCODE_RAM_CYCLES: u32 = 1;

pub(crate) const STORAGE_READ_RAM_CYCLES: u32 = 1;
pub(crate) const STORAGE_READ_LOG_DEMUXER_CYCLES: u32 = 1;
pub(crate) const STORAGE_READ_STORAGE_SORTER_CYCLES: u32 = 1;
pub(crate) const STORAGE_READ_STORAGE_APPLICATION_CYCLES: u32 = 1;

pub(crate) const TRANSIENT_STORAGE_READ_RAM_CYCLES: u32 = 1;
pub(crate) const TRANSIENT_STORAGE_READ_LOG_DEMUXER_CYCLES: u32 = 1;
pub(crate) const TRANSIENT_STORAGE_READ_TRANSIENT_STORAGE_CHECKER_CYCLES: u32 = 1;

pub(crate) const EVENT_RAM_CYCLES: u32 = 1;
pub(crate) const EVENT_LOG_DEMUXER_CYCLES: u32 = 2;
pub(crate) const EVENT_EVENTS_SORTER_CYCLES: u32 = 2;

pub(crate) const STORAGE_WRITE_RAM_CYCLES: u32 = 1;
pub(crate) const STORAGE_WRITE_LOG_DEMUXER_CYCLES: u32 = 2;
pub(crate) const STORAGE_WRITE_STORAGE_SORTER_CYCLES: u32 = 2;
pub(crate) const STORAGE_WRITE_STORAGE_APPLICATION_CYCLES: u32 = 2;

pub(crate) const TRANSIENT_STORAGE_WRITE_RAM_CYCLES: u32 = 1;
pub(crate) const TRANSIENT_STORAGE_WRITE_LOG_DEMUXER_CYCLES: u32 = 2;
pub(crate) const TRANSIENT_STORAGE_WRITE_TRANSIENT_STORAGE_CHECKER_CYCLES: u32 = 2;

pub(crate) const FAR_CALL_RAM_CYCLES: u32 = 1;
pub(crate) const FAR_CALL_STORAGE_SORTER_CYCLES: u32 = 1;
pub(crate) const FAR_CALL_CODE_DECOMMITTER_SORTER_CYCLES: u32 = 1;
pub(crate) const FAR_CALL_LOG_DEMUXER_CYCLES: u32 = 1;

// 5 RAM permutations, because: 1 to read opcode + 2 reads + 2 writes.
// 2 reads and 2 writes are needed because unaligned access is implemented with
// aligned queries.
pub(crate) const UMA_WRITE_RAM_CYCLES: u32 = 5;

// 3 RAM permutations, because: 1 to read opcode + 2 reads.
// 2 reads are needed because unaligned access is implemented with aligned queries.
pub(crate) const UMA_READ_RAM_CYCLES: u32 = 3;

pub(crate) const PRECOMPILE_RAM_CYCLES: u32 = 1;
pub(crate) const PRECOMPILE_LOG_DEMUXER_CYCLES: u32 = 1;

pub(crate) const LOG_DECOMMIT_RAM_CYCLES: u32 = 1;
pub(crate) const LOG_DECOMMIT_DECOMMITTER_SORTER_CYCLES: u32 = 1;

#[derive(Debug)]
pub struct Settings {
    pub default_aa_code_hash: [u8; 32],
    pub evm_interpreter_code_hash: [u8; 32],

    /// Writing to this address in the bootloader's heap suspends execution
    pub hook_address: u32,
}

pub struct VirtualMachine {
    pub world_diff: WorldDiff,

    /// Storing the state in a separate struct is not just cosmetic.
    /// The state couldn't be passed to the world if it was inlined.
    pub state: State,

    pub statistics: CircuitCycleStatistic,
    pub(crate) settings: Settings,

    pub(crate) stack_pool: StackPool,
}

#[derive(Debug, Default)]
pub struct CircuitCycleStatistic {
    pub main_vm_cycles: u32,
    pub ram_permutation_cycles: u32,
    pub storage_application_cycles: u32,
    pub storage_sorter_cycles: u32,
    pub code_decommitter_cycles: u32,
    pub code_decommitter_sorter_cycles: u32,
    pub log_demuxer_cycles: u32,
    pub events_sorter_cycles: u32,
    pub keccak256_cycles: u32,
    pub ecrecover_cycles: u32,
    pub sha256_cycles: u32,
    pub secp256k1_verify_cycles: u32,
    pub transient_storage_checker_cycles: u32,
}

impl VirtualMachine {
    pub fn new(
        address: H160,
        program: Program,
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
                calldata,
                gas,
                program,
                world_before_this_frame,
                stack_pool.get(),
            ),
            settings,
            stack_pool,
            statistics: CircuitCycleStatistic::default(),
        }
    }

    pub fn run(&mut self, world: &mut dyn World) -> ExecutionEnd {
        self.resume_from(0, world)
    }

    pub fn resume_from(&mut self, instruction_number: u16, world: &mut dyn World) -> ExecutionEnd {
        let mut instruction: *const Instruction = self
            .state
            .current_frame
            .program
            .instruction(instruction_number)
            .unwrap();

        unsafe {
            loop {
                let args = &(*instruction).arguments;
                self.track_statistics(instruction);

                if self.state.use_gas(args.get_static_gas_cost()).is_err()
                    || !args.mode_requirements().met(
                        self.state.current_frame.is_kernel,
                        self.state.current_frame.is_static,
                    )
                {
                    instruction = match free_panic(self, world) {
                        Ok(i) => i,
                        Err(e) => return e,
                    };
                    continue;
                }

                #[cfg(feature = "trace")]
                self.print_instruction(instruction);

                if args.predicate().satisfied(&self.state.flags) {
                    instruction = match ((*instruction).handler)(self, instruction, world) {
                        Ok(n) => n,
                        Err(e) => return e,
                    };
                } else {
                    instruction = instruction.add(1);
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
        instruction_number: u16,
        world: &mut dyn World,
        gas_limit: u32,
    ) -> Option<(u32, ExecutionEnd)> {
        let minimum_gas = self.state.total_unspent_gas().saturating_sub(gas_limit);

        let mut instruction: *const Instruction = self
            .state
            .current_frame
            .program
            .instruction(instruction_number)
            .unwrap();

        let end = unsafe {
            loop {
                let args = &(*instruction).arguments;
                self.track_statistics(instruction);

                if self.state.use_gas(args.get_static_gas_cost()).is_err()
                    || !args.mode_requirements().met(
                        self.state.current_frame.is_kernel,
                        self.state.current_frame.is_static,
                    )
                {
                    instruction = match free_panic(self, world) {
                        Ok(i) => i,
                        Err(end) => break end,
                    };
                    continue;
                }

                #[cfg(feature = "trace")]
                self.print_instruction(instruction);

                if args.predicate().satisfied(&self.state.flags) {
                    instruction = match ((*instruction).handler)(self, instruction, world) {
                        Ok(n) => n,
                        Err(end) => break end,
                    };
                } else {
                    instruction = instruction.add(1);
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
    pub(crate) fn push_frame<const CALLING_MODE: u8>(
        &mut self,
        instruction_pointer: *const Instruction,
        code_address: H160,
        program: Program,
        gas: u32,
        stipend: u32,
        exception_handler: u16,
        is_static: bool,
        calldata_heap: HeapId,
        world_before_this_frame: Snapshot,
    ) {
        let mut new_frame = Callframe::new(
            if CALLING_MODE == CallingMode::Delegate as u8 {
                self.state.current_frame.address
            } else {
                code_address
            },
            code_address,
            if CALLING_MODE == CallingMode::Normal as u8 {
                self.state.current_frame.address
            } else if CALLING_MODE == CallingMode::Delegate as u8 {
                self.state.current_frame.caller
            } else {
                // Mimic call
                u256_into_address(self.state.registers[15])
            },
            program,
            self.stack_pool.get(),
            self.state.heaps.allocate(),
            self.state.heaps.allocate(),
            calldata_heap,
            gas,
            stipend,
            exception_handler,
            if CALLING_MODE == CallingMode::Delegate as u8 {
                self.state.current_frame.context_u128
            } else {
                self.state.context_u128
            },
            is_static,
            world_before_this_frame,
        );
        self.state.context_u128 = 0;

        let old_pc = self.state.current_frame.pc_to_u16(instruction_pointer);
        std::mem::swap(&mut new_frame, &mut self.state.current_frame);
        self.state.previous_frames.push((old_pc, new_frame));
    }

    pub(crate) fn pop_frame(&mut self, heap_to_keep: Option<HeapId>) -> Option<FrameRemnant> {
        self.state
            .previous_frames
            .pop()
            .map(|(program_counter, mut frame)| {
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
                    program_counter,
                    exception_handler,
                    snapshot: world_before_this_frame,
                }
            })
    }

    #[cfg(feature = "trace")]
    fn print_instruction(&self, instruction: *const Instruction) {
        print!("{:?}: ", unsafe {
            instruction.offset_from(self.state.current_frame.program.instruction(0).unwrap())
        });
        self.state.registers[1..]
            .iter()
            .zip(1..)
            .for_each(|(&(mut x), i)| {
                if self.state.register_pointer_flags & (1 << i) != 0 {
                    x.0[0] &= 0x00000000_ffffffffu64;
                    x.0[1] &= 0xffffffff_00000000u64;
                }
                print!("{x:?} ")
            });
        print!("{}", self.state.current_frame.gas);
        println!();
    }

    pub(crate) fn start_new_tx(&mut self) {
        self.state.transaction_number = self.state.transaction_number.wrapping_add(1);
        self.world_diff.clear_transient_storage()
    }

    fn track_statistics(&mut self, instruction: *const Instruction) {
        self.statistics.main_vm_cycles += 1;

        let opcode = unsafe { (*instruction).opcode };
        match opcode {
            Opcode::Nop(_)
            | Opcode::Add(_)
            | Opcode::Sub(_)
            | Opcode::Mul(_)
            | Opcode::Div(_)
            | Opcode::Jump(_)
            | Opcode::Binop(_)
            | Opcode::Shift(_)
            | Opcode::Ptr(_) => {
                self.statistics.ram_permutation_cycles += RICH_ADDRESSING_OPCODE_RAM_CYCLES;
            }
            Opcode::Context(_) | Opcode::Ret(_) | Opcode::NearCall(_) => {
                self.statistics.ram_permutation_cycles += AVERAGE_OPCODE_RAM_CYCLES;
            }
            Opcode::Log(LogOpcode::StorageRead) => {
                self.statistics.ram_permutation_cycles += STORAGE_READ_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += STORAGE_READ_LOG_DEMUXER_CYCLES;
                self.statistics.storage_sorter_cycles += STORAGE_READ_STORAGE_SORTER_CYCLES;
            }
            Opcode::Log(LogOpcode::TransientStorageRead) => {
                self.statistics.ram_permutation_cycles += TRANSIENT_STORAGE_READ_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += TRANSIENT_STORAGE_READ_LOG_DEMUXER_CYCLES;
                self.statistics.transient_storage_checker_cycles +=
                    TRANSIENT_STORAGE_READ_TRANSIENT_STORAGE_CHECKER_CYCLES;
            }
            Opcode::Log(LogOpcode::StorageWrite) => {
                self.statistics.ram_permutation_cycles += STORAGE_WRITE_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += STORAGE_WRITE_LOG_DEMUXER_CYCLES;
                self.statistics.storage_sorter_cycles += STORAGE_WRITE_STORAGE_SORTER_CYCLES;
            }
            Opcode::Log(LogOpcode::TransientStorageWrite) => {
                self.statistics.ram_permutation_cycles += TRANSIENT_STORAGE_WRITE_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += TRANSIENT_STORAGE_WRITE_LOG_DEMUXER_CYCLES;
                self.statistics.transient_storage_checker_cycles +=
                    TRANSIENT_STORAGE_WRITE_TRANSIENT_STORAGE_CHECKER_CYCLES;
            }
            Opcode::Log(LogOpcode::ToL1Message) | Opcode::Log(LogOpcode::Event) => {
                self.statistics.ram_permutation_cycles += EVENT_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += EVENT_LOG_DEMUXER_CYCLES;
                self.statistics.events_sorter_cycles += EVENT_EVENTS_SORTER_CYCLES;
            }
            Opcode::Log(LogOpcode::PrecompileCall) => {
                self.statistics.ram_permutation_cycles += PRECOMPILE_RAM_CYCLES;
                self.statistics.log_demuxer_cycles += PRECOMPILE_LOG_DEMUXER_CYCLES;
            }
            Opcode::Log(LogOpcode::Decommit) => {
                // Note, that for decommit the log demuxer circuit is not used.
                self.statistics.ram_permutation_cycles += LOG_DECOMMIT_RAM_CYCLES;
                self.statistics.code_decommitter_sorter_cycles +=
                    LOG_DECOMMIT_DECOMMITTER_SORTER_CYCLES;
            }
            Opcode::FarCall(_) => {
                self.statistics.ram_permutation_cycles += FAR_CALL_RAM_CYCLES;
                self.statistics.code_decommitter_sorter_cycles +=
                    FAR_CALL_CODE_DECOMMITTER_SORTER_CYCLES;
                self.statistics.storage_sorter_cycles += FAR_CALL_STORAGE_SORTER_CYCLES;
                self.statistics.log_demuxer_cycles += FAR_CALL_LOG_DEMUXER_CYCLES;
            }
            Opcode::UMA(
                UMAOpcode::AuxHeapWrite | UMAOpcode::HeapWrite | UMAOpcode::StaticMemoryWrite,
            ) => {
                self.statistics.ram_permutation_cycles += UMA_WRITE_RAM_CYCLES;
            }
            Opcode::UMA(
                UMAOpcode::AuxHeapRead
                | UMAOpcode::HeapRead
                | UMAOpcode::FatPointerRead
                | UMAOpcode::StaticMemoryRead,
            ) => {
                self.statistics.ram_permutation_cycles += UMA_READ_RAM_CYCLES;
            }
            Opcode::Invalid(_) => unreachable!(), // invalid opcodes are never executed
        };
    }
}

pub struct VmSnapshot {
    world_snapshot: ExternalSnapshot,
    state_snapshot: StateSnapshot,
}
