use super::{common::run_next_instruction, pointer::FatPointer, ret};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Immediate1, Register1, Register2,
        RegisterOrImmediate, Source, SourceWriter,
    },
    Instruction, Predicate, State, World,
};
use u256::U256;

pub trait HeapFromState {
    fn get_heap<W: World>(state: &mut State<W>) -> &mut Vec<u8>;
}

pub struct Heap;
impl HeapFromState for Heap {
    fn get_heap<W: World>(state: &mut State<W>) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.heap as usize]
    }
}

pub struct AuxHeap;
impl HeapFromState for AuxHeap {
    fn get_heap<W: World>(state: &mut State<W>) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.aux_heap as usize]
    }
}

fn load<W: World, H: HeapFromState, In: Source, const INCREMENT: bool>(
    state: &mut State<W>,
    instruction: *const Instruction<W>,
) {
    let args = unsafe { &(*instruction).arguments };

    let pointer = In::get(args, state);
    let address = pointer.low_u32();
    let Some(new_bound) = address.checked_add(32) else {
        return ret::panic();
    };

    if !grow_heap::<W, H>(state, new_bound) {
        return ret::panic();
    }

    let heap = H::get_heap(state);
    let value = U256::from_big_endian(&heap[address as usize..new_bound as usize]);
    Register1::set(args, state, value);

    if INCREMENT {
        // TODO zk_evm preserves pointerness here. Should we?
        Register2::set(args, state, pointer + 32)
    }

    run_next_instruction(state, instruction)
}

fn store<W: World, H: HeapFromState, In1: Source, const INCREMENT: bool>(
    state: &mut State<W>,
    instruction: *const Instruction<W>,
) {
    let args = unsafe { &(*instruction).arguments };

    let pointer = In1::get(args, state);
    let value = Register2::get(args, state);

    let address = pointer.low_u32();
    let Some(new_bound) = address.checked_add(32) else {
        return ret::panic();
    };

    if !grow_heap::<W, H>(state, new_bound) {
        return ret::panic();
    }

    let heap = H::get_heap(state);
    value.to_big_endian(&mut heap[address as usize..new_bound as usize]);

    if INCREMENT {
        // TODO zk_evm preserves pointerness here. Should we?
        Register1::set(args, state, pointer + 32)
    }

    run_next_instruction(state, instruction)
}

fn grow_heap<W: World, H: HeapFromState>(state: &mut State<W>, new_bound: u32) -> bool {
    if let Some(growth) = new_bound.checked_sub(H::get_heap(state).len() as u32) {
        // TODO use the proper formula instead
        if state.use_gas(growth) {
            return false;
        }

        // This will not cause frequent reallocations; it allocates in a geometric series like push.
        H::get_heap(state).resize(new_bound as usize, 0);
    }
    true
}

fn load_pointer<W: World, const INCREMENT: bool>(
    state: &mut State<W>,
    instruction: *const Instruction<W>,
) {
    let args = unsafe { &(*instruction).arguments };

    if !Register1::is_fat_pointer(args, state) {
        return ret::panic();
    }
    let input = Register1::get(args, state);
    let pointer = FatPointer::from(input);

    let value = if pointer.offset < pointer.length {
        let heap = &state.heaps[pointer.memory_page as usize];
        let address = (pointer.start + pointer.offset) as usize;
        let mut buffer = [0; 32];
        for (i, byte) in heap[address..(address + 32).min(heap.len())]
            .iter()
            .enumerate()
        {
            buffer[i] = *byte;
        }
        U256::from_big_endian(&buffer)
    } else {
        U256::zero()
    };

    Register1::set(args, state, value);

    if INCREMENT {
        Register2::set_fat_ptr(args, state, input + 32)
    }

    run_next_instruction(state, instruction)
}

use super::monomorphization::*;

impl<W: World> Instruction<W> {
    #[inline(always)]
    pub fn from_load<H: HeapFromState>(
        src: RegisterOrImmediate,
        out: Register1,
        incremented_out: Option<Register2>,
        predicate: Predicate,
    ) -> Self {
        let mut arguments = Arguments::default();
        src.write_source(&mut arguments);
        out.write_destination(&mut arguments);

        let increment = incremented_out.is_some();
        if let Some(out2) = incremented_out {
            out2.write_destination(&mut arguments);
        }
        arguments.predicate = predicate;

        Self {
            handler: monomorphize!(load [W H] match_reg_imm src match_boolean increment),
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
        let mut arguments = Arguments::default();
        src1.write_source(&mut arguments);
        src2.write_source(&mut arguments);

        let increment = incremented_out.is_some();
        if let Some(out) = incremented_out {
            out.write_destination(&mut arguments);
        }
        arguments.predicate = predicate;

        Self {
            handler: monomorphize!(store [W H] match_reg_imm src1 match_boolean increment),
            arguments,
        }
    }

    #[inline(always)]
    pub fn from_load_pointer(
        src: Register1,
        out: Register1,
        incremented_out: Option<Register2>,
        predicate: Predicate,
    ) -> Self {
        let mut arguments = Arguments::default();
        src.write_source(&mut arguments);
        out.write_destination(&mut arguments);

        let increment = incremented_out.is_some();
        if let Some(out2) = incremented_out {
            out2.write_destination(&mut arguments);
        }
        arguments.predicate = predicate;

        Self {
            handler: monomorphize!(load_pointer [W] match_boolean increment),
            arguments,
        }
    }
}
