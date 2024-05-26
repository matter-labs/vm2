use super::stack::StackPool;
use crate::{
    callframe::Callframe, instruction::InstructionResult, instruction_handlers::free_panic,
    Instruction, State, VirtualMachine, World,
};
use arbitrary::Arbitrary;
use std::fmt::Debug;

impl VirtualMachine {
    fn get_first_instruction(&self) -> *const Instruction {
        self.state.current_frame.program.instruction(0).unwrap()
    }

    pub fn run_single_instruction(&mut self, world: &mut dyn World) -> InstructionResult {
        let instruction = self.get_first_instruction();

        unsafe {
            let args = &(*instruction).arguments;
            let Ok(_) = self.state.use_gas(args.get_static_gas_cost()) else {
                return free_panic(self, world);
            };

            if args.predicate.satisfied(&self.state.flags) {
                ((*instruction).handler)(self, instruction, world)
            } else {
                Ok(instruction.add(1))
            }
        }
    }

    pub fn is_in_valid_state(&self) -> bool {
        self.state.is_valid()
    }

    pub fn instruction_is_not_precompile_call(&self) -> bool {
        // Precompilecall is not allowed because it accesses memory multiple times
        // and only needs to work as used by trusted code
        self.state.current_frame.program.raw_first_instruction & 0x7FF != 1056u64
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
                previous_frames: if u.arbitrary()? {
                    vec![(0, Callframe::dummy())]
                } else {
                    vec![]
                },
                heaps: u.arbitrary()?,
                transaction_number: u.arbitrary()?,
                context_u128: u.arbitrary()?,
            },
            settings: u.arbitrary()?,
            world_diff: Default::default(),
            stack_pool: StackPool {},
        })
    }
}

impl Debug for VirtualMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "print useful debugging information here!")
    }
}
