use std::{fmt, sync::Arc};

use primitive_types::U256;
use zksync_vm2_interface::Tracer;

use crate::{
    addressing_modes::Arguments, decode::decode, hash_for_debugging, instruction::ExecutionStatus,
    Instruction, ModeRequirements, Predicate, VirtualMachine, World,
};

/// Compiled EraVM bytecode.
///
/// Cloning this is cheap. It is a handle to memory similar to [`Arc`].
pub struct Program<T, W> {
    // An internal representation that doesn't need two Arcs would be better
    // but it would also require a lot of unsafe, so I made this wrapper to
    // enable changing the internals later.
    code_page: Arc<[U256]>,
    instructions: Arc<[Instruction<T, W>]>,
}

impl<T, W> Clone for Program<T, W> {
    fn clone(&self) -> Self {
        Self {
            code_page: self.code_page.clone(),
            instructions: self.instructions.clone(),
        }
    }
}

impl<T, W> fmt::Debug for Program<T, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEBUGGED_ITEMS: usize = 16;

        let mut s = formatter.debug_struct("Program");
        if self.code_page.len() <= DEBUGGED_ITEMS {
            s.field("code_page", &self.code_page);
        } else {
            s.field("code_page.len", &self.code_page.len())
                .field("code_page.start", &&self.code_page[..DEBUGGED_ITEMS])
                .field("code_page.hash", &hash_for_debugging(&self.code_page));
        }

        if self.instructions.len() <= DEBUGGED_ITEMS {
            s.field("instructions", &self.instructions);
        } else {
            s.field("instructions.len", &self.instructions.len())
                .field("instructions.start", &&self.instructions[..DEBUGGED_ITEMS]);
        }
        s.finish_non_exhaustive()
    }
}

impl<T: Tracer, W: World<T>> Program<T, W> {
    /// Creates a new program.
    pub fn new(bytecode: Vec<u8>, enable_hooks: bool) -> Self {
        let instructions = decode_program(
            &bytecode
                .chunks_exact(8)
                .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
                .collect::<Vec<_>>(),
            enable_hooks,
        );
        let code_page = bytecode
            .chunks_exact(32)
            .map(U256::from_big_endian)
            .collect::<Vec<_>>();
        Self {
            instructions: instructions.into(),
            code_page: code_page.into(),
        }
    }

    /// Creates a new program from `U256` words.
    pub fn from_words(bytecode_words: Vec<U256>, enable_hooks: bool) -> Self {
        let instructions = decode_program(
            &bytecode_words
                .iter()
                .flat_map(|x| x.0.into_iter().rev())
                .collect::<Vec<_>>(),
            enable_hooks,
        );
        Self {
            instructions: instructions.into(),
            code_page: bytecode_words.into(),
        }
    }

    #[doc(hidden)] // should only be used in low-level tests / benchmarks
    pub fn from_raw(instructions: Vec<Instruction<T, W>>, code_page: Vec<U256>) -> Self {
        Self {
            instructions: instructions.into(),
            code_page: code_page.into(),
        }
    }

    pub(crate) fn instruction(&self, n: u16) -> Option<&Instruction<T, W>> {
        self.instructions.get::<usize>(n.into())
    }

    /// Returns a reference to the code page of this program.
    pub fn code_page(&self) -> &[U256] {
        &self.code_page
    }
}

// This implementation compares pointers instead of programs.
//
// That works well enough for the tests that this is written for.
// I don't want to implement PartialEq for Instruction because
// comparing function pointers can work in suprising ways.
impl<T, W> PartialEq for Program<T, W> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.code_page, &other.code_page)
            && Arc::ptr_eq(&self.instructions, &other.instructions)
    }
}

/// "Jump to start" instruction placed at the end of programs exceeding `1 << 16` instructions.
fn jump_to_beginning<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction {
        handler: jump_to_beginning_handler,
        arguments: Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    }
}

fn jump_to_beginning_handler<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    _: &mut W,
    _: &mut T,
) -> ExecutionStatus {
    let first_instruction = vm.state.current_frame.program.instruction(0).unwrap();
    vm.state.current_frame.pc = first_instruction;
    ExecutionStatus::Running
}

fn decode_program<T: Tracer, W: World<T>>(
    raw: &[u64],
    is_bootloader: bool,
) -> Vec<Instruction<T, W>> {
    raw.iter()
        .take(1 << 16)
        .map(|i| decode(*i, is_bootloader))
        .chain(std::iter::once(if raw.len() >= 1 << 16 {
            jump_to_beginning()
        } else {
            Instruction::from_invalid()
        }))
        .collect()
}
