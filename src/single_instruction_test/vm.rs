use super::{heap::Heaps, stack::StackPool};
use crate::{
    addressing_modes::Arguments, callframe::Callframe, fat_pointer::FatPointer,
    instruction::ExecutionStatus, HeapId, Instruction, ModeRequirements, Predicate, Settings,
    State, VirtualMachine, World,
};
use arbitrary::Arbitrary;
use eravm_stable_interface::Tracer;
use std::fmt::Debug;
use u256::U256;

impl<T: Tracer, W> VirtualMachine<T, W> {
    pub fn run_single_instruction(&mut self, world: &mut W, tracer: &mut T) -> ExecutionStatus {
        unsafe { ((*self.state.current_frame.pc).handler)(self, world, tracer) }
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

impl<'a, T: Tracer, W: World<T>> Arbitrary<'a> for VirtualMachine<T, W> {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let current_frame: Callframe<T, W> = u.arbitrary()?;

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
                previous_frames: vec![Callframe::dummy()],
                heaps,
                transaction_number: u.arbitrary()?,
                context_u128: u.arbitrary()?,
            },
            settings: u.arbitrary()?,
            world_diff: Default::default(),
            stack_pool: StackPool {},
            panic: Box::new(Instruction::from_panic(
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            )),
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

impl<T, W> Debug for VirtualMachine<T, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "print useful debugging information here!")
    }
}
