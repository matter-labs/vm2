use super::{common::instruction_boilerplate, pointer::FatPointer};
use crate::{
    addressing_modes::{Destination, Register1, Register2, Source},
    Instruction, State,
};
use u256::U256;

trait HeapFromState {
    fn get_heap(state: &mut State) -> &mut Vec<u8>;
}

struct Heap;
impl HeapFromState for Heap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.heap as usize]
    }
}

struct AuxHeap;
impl HeapFromState for AuxHeap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.aux_heap as usize]
    }
}

fn load<In: Source, H: HeapFromState, const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        let pointer = In::get(args, state);
        let address = pointer.low_u32();
        let Some(new_bound) = address.checked_add(32) else {
            return; // TODO panic
        };
        let heap = H::get_heap(state);

        grow_heap(heap, new_bound);

        let value = U256::from_big_endian(&heap[address as usize..new_bound as usize]);
        Register1::set(args, state, value);

        if INCREMENT {
            // TODO zk_evm preserves pointerness here. Should we?
            Register2::set(args, state, pointer + 32)
        }
    })
}

fn store<In1: Source, H: HeapFromState, const SWAP: bool, const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        let (pointer, value) = {
            let a = In1::get(args, state);
            let b = Register2::get(args, state);
            if SWAP {
                (b, a)
            } else {
                (a, b)
            }
        };

        let address = pointer.low_u32();
        let Some(new_bound) = address.checked_add(32) else {
            return; // TODO panic
        };
        let heap = H::get_heap(state);

        grow_heap(heap, new_bound);

        value.to_big_endian(&mut heap[address as usize..new_bound as usize]);

        if INCREMENT {
            // TODO zk_evm preserves pointerness here. Should we?
            Register1::set(args, state, pointer + 32)
        }
    })
}

fn grow_heap(heap: &mut Vec<u8>, new_bound: u32) {
    if new_bound as usize > heap.len() {
        // This will not cause frequent reallocations; it allocates in a geometric series like push.
        heap.resize(new_bound as usize, 0);

        // TODO pay for growth
    }
}

fn load_pointer<In: Source, H: HeapFromState, const INCREMENT: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        if !In::is_fat_pointer(args, state) {
            return; // TODO panic
        }
        let input = In::get(args, state);
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
    })
}
