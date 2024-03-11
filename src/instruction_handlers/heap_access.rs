use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Immediate1, Register1, Register2,
        RegisterOrImmediate, Source,
    },
    fat_pointer::FatPointer,
    state::{InstructionResult, Panic},
    Instruction, Predicate, State,
};
use u256::U256;

pub trait HeapFromState {
    fn get_heap(state: &mut State) -> &mut Vec<u8>;
}

pub struct Heap;
impl HeapFromState for Heap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.heap as usize]
    }
}

pub struct AuxHeap;
impl HeapFromState for AuxHeap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.aux_heap as usize]
    }
}

/// The last address to which 32 can be added without overflow.
const LAST_ADDRESS: u32 = u32::MAX - 32;

fn load<H: HeapFromState, In: Source, const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
        let pointer = In::get(args, state);
        if In::is_fat_pointer(args, state) {
            return Err(Panic::IncorrectPointerTags);
        }
        if pointer > LAST_ADDRESS.into() {
            return Err(Panic::AccessingTooLargeHeapAddress);
        }

        let address = pointer.low_u32();

        // The size check above ensures this never overflows
        let new_bound = address + 32;

        grow_heap::<H>(state, new_bound)?;

        let heap = H::get_heap(state);
        let value = U256::from_big_endian(&heap[address as usize..new_bound as usize]);
        Register1::set(args, state, value);

        if INCREMENT {
            Register2::set(args, state, pointer + 32)
        }

        Ok(())
    })
}

fn store<H: HeapFromState, In: Source, const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
        let pointer = In::get(args, state);
        if In::is_fat_pointer(args, state) {
            return Err(Panic::IncorrectPointerTags);
        }
        if pointer > LAST_ADDRESS.into() {
            return Err(Panic::AccessingTooLargeHeapAddress);
        }

        let value = Register2::get(args, state);

        let address = pointer.low_u32();

        // The size check above ensures this never overflows
        let new_bound = address + 32;

        grow_heap::<H>(state, new_bound)?;

        let heap = H::get_heap(state);
        value.to_big_endian(&mut heap[address as usize..new_bound as usize]);

        if INCREMENT {
            Register1::set(args, state, pointer + 32)
        }

        Ok(())
    })
}

pub fn grow_heap<H: HeapFromState>(state: &mut State, new_bound: u32) -> Result<(), Panic> {
    if let Some(growth) = new_bound.checked_sub(H::get_heap(state).len() as u32) {
        state.use_gas(growth)?;

        // This will not cause frequent reallocations; it allocates in a geometric series like push.
        H::get_heap(state).resize(new_bound as usize, 0);
    }
    Ok(())
}

fn load_pointer<const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(state, instruction, |state, args| {
        if !Register1::is_fat_pointer(args, state) {
            return Err(Panic::IncorrectPointerTags);
        }
        let input = Register1::get(args, state);
        let pointer = FatPointer::from(input);

        let heap = &state.heaps[pointer.memory_page as usize];

        // start + offset could be past the end of the fat pointer
        // any bytes past the end are read as zero
        let start = pointer.start.saturating_add(pointer.offset);
        let Some(end) = start.checked_add(32) else {
            return Err(Panic::PointerOffsetTooLarge);
        };
        let mut buffer = [0; 32];
        for (i, addr) in (start..end.min(pointer.start + pointer.length)).enumerate() {
            buffer[i] = heap[addr as usize];
        }

        Register1::set(args, state, U256::from_big_endian(&buffer));

        if INCREMENT {
            if pointer.offset > LAST_ADDRESS {
                return Err(Panic::PointerOffsetOverflows);
            }
            Register2::set_fat_ptr(args, state, input + 32)
        }

        Ok(())
    })
}

use super::{common::instruction_boilerplate_with_panic, monomorphization::*};

impl Instruction {
    #[inline(always)]
    pub fn from_load<H: HeapFromState>(
        src: RegisterOrImmediate,
        out: Register1,
        incremented_out: Option<Register2>,
        predicate: Predicate,
    ) -> Self {
        let mut arguments = Arguments::new(predicate, 7)
            .write_source(&src)
            .write_destination(&out);

        let increment = incremented_out.is_some();
        if let Some(out2) = incremented_out {
            out2.write_destination(&mut arguments);
        }

        Self {
            handler: monomorphize!(load [H] match_reg_imm src match_boolean increment),
            arguments,
        }
    }

    #[inline(always)]
    pub fn from_store<H: HeapFromState>(
        src1: RegisterOrImmediate,
        src2: Register2,
        incremented_out: Option<Register1>,
        predicate: Predicate,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(store [H] match_reg_imm src1 match_boolean increment),
            arguments: Arguments::new(predicate, 13)
                .write_source(&src1)
                .write_source(&src2)
                .write_destination(&incremented_out),
        }
    }

    #[inline(always)]
    pub fn from_load_pointer(
        src: Register1,
        out: Register1,
        incremented_out: Option<Register2>,
        predicate: Predicate,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(load_pointer match_boolean increment),
            arguments: Arguments::new(predicate, 7)
                .write_source(&src)
                .write_destination(&out)
                .write_destination(&incremented_out),
        }
    }
}
