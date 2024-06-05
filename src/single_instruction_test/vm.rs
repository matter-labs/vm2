use super::stack::StackPool;
use crate::{
    callframe::Callframe, instruction::InstructionResult, instruction_handlers::free_panic,
    Instruction, Settings, State, VirtualMachine, World,
};
use arbitrary::Arbitrary;
use std::fmt::Debug;
use u256::U256;

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

    pub fn instruction_is_far_call(&self) -> bool {
        let opcode = self.state.current_frame.program.raw_first_instruction & 0x7FF;
        1057 <= opcode && opcode <= 1068
    }
}

impl<'a> Arbitrary<'a> for VirtualMachine {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let mut registers = [U256::zero(); 16];
        for r in &mut registers[1..] {
            *r = u.arbitrary()?;
        }
        let mut register_pointer_flags = u.arbitrary()?;
        register_pointer_flags &= !1;

        Ok(Self {
            state: State {
                registers,
                register_pointer_flags,
                flags: u.arbitrary()?,
                current_frame: u.arbitrary()?,
                // Exiting the final frame is different in vm2 on purpose,
                // so always generate two frames to avoid that.
                previous_frames: vec![(0, Callframe::dummy())],
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

impl<'a> Arbitrary<'a> for Settings {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        // Only contract hashes that begin 1, 0 are valid
        let mut default_aa_code_hash = [0u8; 32];
        default_aa_code_hash[0] = 1;
        u.fill_buffer(&mut default_aa_code_hash[2..])?;

        let mut evm_interpreter_code_hash = [0u8; 32];
        evm_interpreter_code_hash[0] = 1;
        u.fill_buffer(&mut evm_interpreter_code_hash[2..])?;

        Ok(Self {
            default_aa_code_hash,
            evm_interpreter_code_hash,
            hook_address: 0, // Doesn't matter; we don't decode in bootloader mode
        })
    }
}

impl Debug for VirtualMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "print useful debugging information here!")
    }
}
