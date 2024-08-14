use super::common::{instruction_boilerplate, instruction_boilerplate_ext, NotifyTracer};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Immediate1, Register1, Register2,
        RegisterOrImmediate, Source,
    },
    fat_pointer::FatPointer,
    instruction::ExecutionStatus,
    state::State,
    ExecutionEnd, HeapId, Instruction, VirtualMachine, World,
};
use eravm_stable_interface::{opcodes, Tracer};
use std::ops::Range;
use u256::U256;

pub trait HeapInterface {
    fn read_u256(&self, start_address: u32) -> U256;
    fn read_u256_partially(&self, range: Range<u32>) -> U256;
    fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8>;
}

pub trait HeapFromState {
    fn get_heap<T>(state: &State<T>) -> HeapId;
    fn get_heap_size<T>(state: &mut State<T>) -> &mut u32;
    type Read: NotifyTracer;
    type Write: NotifyTracer;
}

pub struct Heap;
impl HeapFromState for Heap {
    fn get_heap<T>(state: &State<T>) -> HeapId {
        state.current_frame.heap
    }
    fn get_heap_size<T>(state: &mut State<T>) -> &mut u32 {
        &mut state.current_frame.heap_size
    }
    type Read = opcodes::HeapRead;
    type Write = opcodes::HeapWrite;
}

pub struct AuxHeap;
impl HeapFromState for AuxHeap {
    fn get_heap<T>(state: &State<T>) -> HeapId {
        state.current_frame.aux_heap
    }
    fn get_heap_size<T>(state: &mut State<T>) -> &mut u32 {
        &mut state.current_frame.aux_heap_size
    }
    type Read = opcodes::AuxHeapRead;
    type Write = opcodes::AuxHeapWrite;
}

/// The last address to which 32 can be added without overflow.
const LAST_ADDRESS: u32 = u32::MAX - 32;

fn load<T: Tracer, H: HeapFromState, In: Source, const INCREMENT: bool>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<H::Read, _>(vm, world, tracer, |vm, args, _| {
        // Pointers need not be masked here even though we do not care about them being pointers.
        // They will panic, though because they are larger than 2^32.
        let (pointer, _) = In::get_with_pointer_flag(args, &mut vm.state);

        let address = pointer.low_u32();

        let new_bound = address.wrapping_add(32);
        if grow_heap::<_, H>(&mut vm.state, new_bound).is_err() {
            vm.state.current_frame.pc = &*vm.panic;
            return;
        };

        // The heap is always grown even when the index nonsensical.
        // TODO PLA-974 revert to not growing the heap on failure as soon as zk_evm is fixed
        if pointer > LAST_ADDRESS.into() {
            let _ = vm.state.use_gas(u32::MAX);
            vm.state.current_frame.pc = &*vm.panic;
            return;
        }

        let heap = H::get_heap(&vm.state);
        let value = vm.state.heaps[heap].read_u256(address);
        Register1::set(args, &mut vm.state, value);

        if INCREMENT {
            Register2::set(args, &mut vm.state, pointer + 32)
        }
    })
}

fn store<
    T: Tracer,
    H: HeapFromState,
    In: Source,
    const INCREMENT: bool,
    const HOOKING_ENABLED: bool,
>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate_ext::<H::Write, _>(vm, world, tracer, |vm, args, _| {
        // Pointers need not be masked here even though we do not care about them being pointers.
        // They will panic, though because they are larger than 2^32.
        let (pointer, _) = In::get_with_pointer_flag(args, &mut vm.state);

        let address = pointer.low_u32();
        let value = Register2::get(args, &mut vm.state);

        let new_bound = address.wrapping_add(32);
        if grow_heap::<_, H>(&mut vm.state, new_bound).is_err() {
            vm.state.current_frame.pc = &*vm.panic;
            return ExecutionStatus::Running;
        }

        // The heap is always grown even when the index nonsensical.
        // TODO PLA-974 revert to not growing the heap on failure as soon as zk_evm is fixed
        if pointer > LAST_ADDRESS.into() {
            let _ = vm.state.use_gas(u32::MAX);
            vm.state.current_frame.pc = &*vm.panic;
            return ExecutionStatus::Running;
        }

        let heap = H::get_heap(&vm.state);
        vm.state.heaps.write_u256(heap, address, value);

        if INCREMENT {
            Register1::set(args, &mut vm.state, pointer + 32)
        }

        if HOOKING_ENABLED && address == vm.settings.hook_address {
            ExecutionStatus::Stopped(ExecutionEnd::SuspendedOnHook(value.as_u32()))
        } else {
            ExecutionStatus::Running
        }
    })
}

/// Pays for more heap space. Doesn't acually grow the heap.
/// That distinction is necessary because the bootloader gets u32::MAX heap for free.
pub fn grow_heap<T: Tracer, H: HeapFromState>(
    state: &mut State<T>,
    new_bound: u32,
) -> Result<(), ()> {
    let already_paid = H::get_heap_size(state);
    if *already_paid < new_bound {
        let to_pay = new_bound - *already_paid;
        *already_paid = new_bound;
        state.use_gas(to_pay)?;
    }

    Ok(())
}

fn load_pointer<T: Tracer, const INCREMENT: bool>(
    vm: &mut VirtualMachine<T>,
    world: &mut dyn World<T>,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<opcodes::PointerRead, _>(vm, world, tracer, |vm, args, _| {
        let (input, input_is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);
        if !input_is_pointer {
            vm.state.current_frame.pc = &*vm.panic;
            return;
        }
        let pointer = FatPointer::from(input);

        // Usually, we just read zeroes instead of out-of-bounds bytes
        // but if offset + 32 is not representable, we panic, even if we could've read some bytes.
        // This is not a bug, this is how it must work to be backwards compatible.
        if pointer.offset > LAST_ADDRESS {
            vm.state.current_frame.pc = &*vm.panic;
            return;
        };

        let start = pointer.start + pointer.offset.min(pointer.length);
        let end = start.saturating_add(32).min(pointer.start + pointer.length);

        let value = vm.state.heaps[pointer.memory_page].read_u256_partially(start..end);
        Register1::set(args, &mut vm.state, value);

        if INCREMENT {
            // This addition does not overflow because we checked that the offset is small enough above.
            Register2::set_fat_ptr(args, &mut vm.state, input + 32)
        }
    })
}

use super::monomorphization::*;

impl<T: Tracer> Instruction<T> {
    #[inline(always)]
    pub fn from_load<H: HeapFromState>(
        src: RegisterOrImmediate,
        out: Register1,
        incremented_out: Option<Register2>,
        arguments: Arguments,
    ) -> Self {
        let mut arguments = arguments.write_source(&src).write_destination(&out);

        let increment = incremented_out.is_some();
        if let Some(out2) = incremented_out {
            out2.write_destination(&mut arguments);
        }

        Self {
            handler: monomorphize!(load [T H] match_reg_imm src match_boolean increment),
            arguments,
        }
    }

    #[inline(always)]
    pub fn from_store<H: HeapFromState>(
        src1: RegisterOrImmediate,
        src2: Register2,
        incremented_out: Option<Register1>,
        arguments: Arguments,
        should_hook: bool,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(store [T H] match_reg_imm src1 match_boolean increment match_boolean should_hook),
            arguments: arguments
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
        arguments: Arguments,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(load_pointer [T] match_boolean increment),
            arguments: arguments
                .write_source(&src)
                .write_destination(&out)
                .write_destination(&incremented_out),
        }
    }
}
