use super::{common::instruction_boilerplate_with_panic, PANIC};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Immediate1, Register1, Register2,
        RegisterOrImmediate, Source,
    },
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    state::State,
    Instruction, Predicate, VirtualMachine,
};
use u256::U256;

pub trait HeapFromState {
    fn get_heap(state: &mut State) -> &mut Vec<u8>;
}

pub struct Heap;
impl HeapFromState for Heap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.heap]
    }
}

pub struct AuxHeap;
impl HeapFromState for AuxHeap {
    fn get_heap(state: &mut State) -> &mut Vec<u8> {
        &mut state.heaps[state.current_frame.aux_heap]
    }
}

/// The last address to which 32 can be added without overflow.
const LAST_ADDRESS: u32 = u32::MAX - 32;

fn load<H: HeapFromState, In: Source, const INCREMENT: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        let pointer = In::get(args, &mut vm.state);
        if In::is_fat_pointer(args, &mut vm.state) {
            return Ok(&PANIC);
        }
        if pointer > LAST_ADDRESS.into() {
            let _ = vm.state.use_gas(u32::MAX);
            return Ok(&PANIC);
        }

        let address = pointer.low_u32();

        // The size check above ensures this never overflows
        let new_bound = address + 32;

        if grow_heap::<H>(&mut vm.state, new_bound).is_err() {
            return Ok(&PANIC);
        };

        let heap = H::get_heap(&mut vm.state);
        let value = U256::from_big_endian(&heap[address as usize..new_bound as usize]);
        Register1::set(args, &mut vm.state, value);

        if INCREMENT {
            Register2::set(args, &mut vm.state, pointer + 32)
        }

        continue_normally
    })
}

fn store<H: HeapFromState, In: Source, const INCREMENT: bool, const HOOKING_ENABLED: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        let pointer = In::get(args, &mut vm.state);
        if In::is_fat_pointer(args, &mut vm.state) {
            return Ok(&PANIC);
        }
        if pointer > LAST_ADDRESS.into() {
            let _ = vm.state.use_gas(u32::MAX);
            return Ok(&PANIC);
        }

        let value = Register2::get(args, &mut vm.state);

        let address = pointer.low_u32();

        // The size check above ensures this never overflows
        let new_bound = address + 32;

        if grow_heap::<H>(&mut vm.state, new_bound).is_err() {
            return Ok(&PANIC);
        }

        let heap = H::get_heap(&mut vm.state);
        value.to_big_endian(&mut heap[address as usize..new_bound as usize]);

        if INCREMENT {
            Register1::set(args, &mut vm.state, pointer + 32)
        }

        if HOOKING_ENABLED && address == vm.settings.hook_address {
            vm.world.handle_hook(value.as_u32(), &mut vm.state);
        }

        continue_normally
    })
}

pub fn grow_heap<H: HeapFromState>(state: &mut State, new_bound: u32) -> Result<(), ()> {
    if let Some(growth) = new_bound.checked_sub(H::get_heap(state).len() as u32) {
        state.use_gas(growth)?;

        // This will not cause frequent reallocations; it allocates in a geometric series like push.
        H::get_heap(state).resize(new_bound as usize, 0);
    }
    Ok(())
}

fn load_pointer<const INCREMENT: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        if !Register1::is_fat_pointer(args, &mut vm.state) {
            return Ok(&PANIC);
        }
        let input = Register1::get(args, &mut vm.state);
        let pointer = FatPointer::from(input);

        let heap = &vm.state.heaps[pointer.memory_page];

        // Usually, we just read zeroes instead of out-of-bounds bytes
        // but if offset + 32 is not representable, we panic, even if we could've read some bytes.
        // This is not a bug, this is how it must work to be backwards compatible.
        if pointer.offset > LAST_ADDRESS {
            return Ok(&PANIC);
        };

        let mut buffer = [0; 32];
        if pointer.offset < pointer.length {
            let start = pointer.start + pointer.offset;
            let end = start.saturating_add(32).min(pointer.start + pointer.length);

            for (i, addr) in (start..end).enumerate() {
                buffer[i] = heap[addr as usize];
            }
        }

        Register1::set(args, &mut vm.state, U256::from_big_endian(&buffer));

        if INCREMENT {
            // This addition does not overflow because we checked that the offset is small enough above.
            Register2::set_fat_ptr(args, &mut vm.state, input + 32)
        }

        continue_normally
    })
}

use super::monomorphization::*;

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
        should_hook: bool,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(store [H] match_reg_imm src1 match_boolean increment match_boolean should_hook),
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
