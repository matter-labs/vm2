use crate::{
    decommit::address_into_u256, instruction_handlers::free_panic, modified_world::ModifiedWorld,
    rollback::Rollback, state::State, ExecutionEnd, Instruction, World,
};
use u256::H160;

pub struct Settings {
    pub default_aa_code_hash: [u8; 32],
    pub evm_interpreter_code_hash: [u8; 32],

    /// Writing to this address on the heap in the bootloader causes a call to
    /// the handle_hook method of the provided [World].
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
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        settings: Settings,
    ) -> Self {
        let mut world = ModifiedWorld::new(world);
        let world_before_this_frame = world.snapshot();

        let program = world.initial_decommit(address_into_u256(address));

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
        let mut instruction: *const Instruction =
            &self.state.current_frame.program.instructions()[0];

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
                {
                    print!(
                        "{:?}: ",
                        instruction.offset_from(&self.state.current_frame.program[0])
                    );
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
}
