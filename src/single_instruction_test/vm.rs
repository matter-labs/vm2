use std::fmt::Debug;

use arbitrary::Arbitrary;

use crate::{
    instruction::InstructionResult, instruction_handlers::free_panic, Instruction, State,
    VirtualMachine, World,
};

impl VirtualMachine {
    pub fn get_first_instruction(&self) -> *const Instruction {
        self.state.current_frame.program.instruction(0).unwrap()
    }

    pub fn run_single_instruction(
        &mut self,
        instruction: *const Instruction,
        world: &mut dyn World,
    ) -> InstructionResult {
        unsafe {
            let args = &(*instruction).arguments;
            let Ok(_) = self.state.use_gas(args.get_static_gas_cost()) else {
                return free_panic(self, world);
            };

            return if args.predicate.satisfied(&self.state.flags) {
                ((*instruction).handler)(self, instruction, world)
            } else {
                Ok(instruction.add(1))
            };
        }
    }
}

impl<'a> Arbitrary<'a> for VirtualMachine {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            state: State {
                registers: u.arbitrary()?,
                register_pointer_flags: u.arbitrary()?,
                flags: u.arbitrary()?,
                current_frame: u.arbitrary()?,
                previous_frames: vec![], // TODO
                heaps: u.arbitrary()?,
                transaction_number: u.arbitrary()?,
                context_u128: u.arbitrary()?,
            },
            settings: u.arbitrary()?,
            world_diff: Default::default(),
            stack_pool: u.arbitrary()?,
        })
    }
}

impl Debug for VirtualMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "print useful debugging information here!")
    }
}
