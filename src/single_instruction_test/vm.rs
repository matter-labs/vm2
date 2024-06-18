use super::{heap::Heaps, stack::StackPool};
use crate::{
    callframe::Callframe, fat_pointer::FatPointer, instruction::InstructionResult,
    instruction_handlers::free_panic, HeapId, Instruction, Settings, State, VirtualMachine, World,
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

            if self.state.use_gas(args.get_static_gas_cost()).is_err()
                || !args.mode_requirements().met(
                    self.state.current_frame.is_kernel,
                    self.state.current_frame.is_static,
                )
            {
                return free_panic(self, world);
            }

            if args.predicate().satisfied(&self.state.flags) {
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
        // TODO PLA-972 implement StaticMemoryRead/Write
        if (1096..=1103).contains(&self.current_opcode()) {
            return false;
        }

        // Precompilecall is not allowed because it accesses memory multiple times
        // and only needs to work as used by trusted code
        self.current_opcode() != 1056u64
    }

    pub fn instruction_is_far_call(&self) -> bool {
        (1057..=1068).contains(&self.current_opcode())
    }

    fn current_opcode(&self) -> u64 {
        self.state.current_frame.program.raw_first_instruction & 0x7FF
    }
}

impl<'a> Arbitrary<'a> for VirtualMachine {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let current_frame: Callframe = u.arbitrary()?;

        let mut registers = [U256::zero(); 16];
        let mut register_pointer_flags = 0;

        for (i, register) in registers.iter_mut().enumerate().skip(1) {
            let (value, is_pointer) = arbitrary_register_value(
                u,
                current_frame.calldata_heap,
                current_frame.heap.to_u32() - 2,
            )?;
            *register = value;
            register_pointer_flags |= (is_pointer as u16) << i;
        }

        let heaps = Heaps::from_id(current_frame.heap, u)?;

        Ok(Self {
            state: State {
                registers,
                register_pointer_flags,
                flags: u.arbitrary()?,
                current_frame,
                // Exiting the final frame is different in vm2 on purpose,
                // so always generate two frames to avoid that.
                previous_frames: vec![(0, Callframe::dummy())],
                heaps,
                transaction_number: u.arbitrary()?,
                context_u128: u.arbitrary()?,
            },
            settings: u.arbitrary()?,
            world_diff: Default::default(),
            stack_pool: StackPool {},
        })
    }
}

/// Generates a pointer or non-pointer value.
/// The pointers always point to the calldata heap or a heap larger than the base page.
/// This is because heap < base_page in zk_evm means the same as heap == calldata_heap in vm2.
pub(crate) fn arbitrary_register_value(
    u: &mut arbitrary::Unstructured,
    calldata_heap: HeapId,
    base_page: u32,
) -> arbitrary::Result<(U256, bool)> {
    Ok(if u.arbitrary()? {
        (
            (U256::from(u.arbitrary::<u128>()?) << 128)
                | FatPointer {
                    offset: u.arbitrary()?,
                    memory_page: if u.arbitrary()? {
                        // generate a pointer to calldata
                        calldata_heap
                    } else {
                        // generate a pointer to return data
                        HeapId::from_u32_unchecked(u.int_in_range(base_page..=u32::MAX)?)
                    },
                    start: u.arbitrary()?,
                    length: u.arbitrary()?,
                }
                .into_u256(),
            true,
        )
    } else {
        // generate a value
        (u.arbitrary()?, false)
    })
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
