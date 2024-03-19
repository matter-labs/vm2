use crate::{
    decommit::{address_into_u256, decommit},
    instruction::Panic,
    instruction_handlers::ret_panic,
    modified_world::ModifiedWorld,
    rollback::Rollback,
    state::State,
    ExecutionEnd, Instruction, World,
};
use u256::{H160, U256};

pub struct Settings {
    pub default_aa_code_hash: U256,

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
        mut world: Box<dyn World>,
        address: H160,
        caller: H160,
        calldata: Vec<u8>,
        gas: u32,
        settings: Settings,
    ) -> Self {
        let (program, code_page, _) = decommit(
            &mut *world,
            address_into_u256(address),
            settings.default_aa_code_hash,
        )
        .unwrap();

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
                code_page,
                world_before_this_frame,
            ),
            settings,
        }
    }

    pub fn run(&mut self) -> ExecutionEnd {
        let mut instruction: *const Instruction = &self.state.current_frame.program[0];

        unsafe {
            loop {
                #[cfg(trace)]
                {
                    print!(
                        "{:?}: ",
                        instruction.offset_from(&self.state.current_frame.program[0])
                    );
                    self.state.registers[1..]
                        .iter()
                        .for_each(|x| print!("{x:?} "));
                    println!();
                }

                let args = &(*instruction).arguments;
                let Ok(_) = self.state.use_gas(args.get_static_gas_cost()) else {
                    instruction = match ret_panic(self, Panic::OutOfGas) {
                        Ok(i) => i,
                        Err(e) => return e,
                    };
                    continue;
                };

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
