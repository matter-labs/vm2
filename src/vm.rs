use crate::{
    instruction_handlers::free_panic, modified_world::ModifiedWorld, state::State, ExecutionEnd,
    Instruction, Program, World,
};
use u256::H160;

pub struct Settings {
    pub default_aa_code_hash: [u8; 32],
    pub evm_interpreter_code_hash: [u8; 32],

    /// Writing to this address in the bootloader's heap suspends execution
    pub hook_address: u32,
}

pub struct VirtualMachine {
    pub world: ModifiedWorld,

    /// Storing the state in a separate struct is not just cosmetic.
    /// The state couldn't be passed to the world if it was inlined.
    pub state: State,

    pub(crate) settings: Settings,
}

impl VirtualMachine {
    pub fn new(
        world: Box<dyn World>,
        address: H160,
        program: Program,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        settings: Settings,
    ) -> Self {
        let world = ModifiedWorld::new(world);
        let world_before_this_frame = world.snapshot();

        Self {
            world,
            state: State::new(
                address,
                caller,
                calldata,
                gas,
                program,
                world_before_this_frame,
            ),
            settings,
        }
    }

    pub fn run(&mut self) -> ExecutionEnd {
        self.resume_from(0)
    }

    pub fn resume_from(&mut self, instruction_number: u16) -> ExecutionEnd {
        let mut instruction: *const Instruction =
            &self.state.current_frame.program.instructions()[instruction_number as usize];

        unsafe {
            loop {
                let args = &(*instruction).arguments;
                let Ok(_) = self.state.use_gas(args.get_static_gas_cost()) else {
                    instruction = match free_panic(self) {
                        Ok(i) => i,
                        Err(e) => return e,
                    };
                    continue;
                };

                #[cfg(trace)]
                self.print_instruction(instruction);

                if args.predicate.satisfied(&self.state.flags) {
                    instruction = match ((*instruction).handler)(self, instruction) {
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
        gas_limit: u32,
    ) -> Option<(u32, ExecutionEnd)> {
        let minimum_gas = self.state.total_unspent_gas().saturating_sub(gas_limit);

        let mut instruction: *const Instruction =
            &self.state.current_frame.program.instructions()[instruction_number as usize];

        let end = unsafe {
            loop {
                let args = &(*instruction).arguments;
                let Ok(_) = self.state.use_gas(args.get_static_gas_cost()) else {
                    instruction = match free_panic(self) {
                        Ok(i) => i,
                        Err(end) => break end,
                    };
                    continue;
                };

                #[cfg(trace)]
                self.print_instruction(instruction);

                if args.predicate.satisfied(&self.state.flags) {
                    instruction = match ((*instruction).handler)(self, instruction) {
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

    #[cfg(trace)]
    fn print_instruction(&self, instruction: *const Instruction) {
        print!("{:?}: ", unsafe {
            instruction.offset_from(&self.state.current_frame.program.instructions()[0])
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
}
