use crate::heap::HeapId;
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

    pub(crate) settings: Settings,

    pub(crate) stack_pool: StackPool,
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
        assert!(self.state.previous_frames.is_empty());
        VmSnapshot {
            world_snapshot: self.world_diff.external_snapshot(),
            state_snapshot: self.state.clone(),
        }
    }

    /// Returns the VM to the state it was in when the snapshot was created.
    /// # Panics
    /// Rolling back snapshots in anything but LIFO order may panic.
    pub fn rollback(&mut self, snapshot: VmSnapshot) {
        self.world_diff.external_rollback(snapshot.world_snapshot);
        self.state = snapshot.state_snapshot;
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
        return;
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
}

pub struct VmSnapshot {
    world_snapshot: ExternalSnapshot,
    state_snapshot: State,
}
