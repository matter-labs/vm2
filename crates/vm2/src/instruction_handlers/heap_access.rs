use primitive_types::U256;
use zksync_vm2_interface::{opcodes, HeapId, OpcodeType, Tracer};

use super::{
    common::{boilerplate, full_boilerplate},
    monomorphization::{match_boolean, match_reg_imm, monomorphize, parameterize},
    ret::spontaneous_panic,
};
use crate::{
    addressing_modes::{
        Arguments, Destination, DestinationWriter, Immediate1, Register1, Register2,
        RegisterOrImmediate, Source,
    },
    fat_pointer::FatPointer,
    instruction::ExecutionStatus,
    state::State,
    ExecutionEnd, Instruction, VirtualMachine, World,
};

pub(crate) trait HeapFromState {
    type Read: OpcodeType;
    type Write: OpcodeType;

    fn get_heap<T, W>(state: &State<T, W>) -> HeapId;
    fn get_heap_size<T, W>(state: &mut State<T, W>) -> &mut u32;
}

pub(crate) struct Heap;

impl HeapFromState for Heap {
    type Read = opcodes::HeapRead;
    type Write = opcodes::HeapWrite;

    fn get_heap<T, W>(state: &State<T, W>) -> HeapId {
        state.current_frame.heap
    }

    fn get_heap_size<T, W>(state: &mut State<T, W>) -> &mut u32 {
        &mut state.current_frame.heap_size
    }
}

pub(crate) struct AuxHeap;

impl HeapFromState for AuxHeap {
    type Read = opcodes::AuxHeapRead;
    type Write = opcodes::AuxHeapWrite;

    fn get_heap<T, W>(state: &State<T, W>) -> HeapId {
        state.current_frame.aux_heap
    }

    fn get_heap_size<T, W>(state: &mut State<T, W>) -> &mut u32 {
        &mut state.current_frame.aux_heap_size
    }
}

/// The last address to which 32 can be added without overflow.
const LAST_ADDRESS: u32 = u32::MAX - 32;

// Necessary because the obvious code compiles to a comparison of two 256-bit numbers.
#[inline(always)]
fn bigger_than_last_address(x: U256) -> bool {
    x.0[0] > LAST_ADDRESS.into() || x.0[1] != 0 || x.0[2] != 0 || x.0[3] != 0
}

fn load<T: Tracer, W: World<T>, H: HeapFromState, In: Source, const INCREMENT: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<H::Read, _, _>(vm, world, tracer, |vm, args| {
        // Pointers need not be masked here even though we do not care about them being pointers.
        // They will panic, though because they are larger than 2^32.
        let (pointer, _) = In::get_with_pointer_flag(args, &mut vm.state);

        let address = pointer.low_u32();

        let new_bound = address.wrapping_add(32);
        if grow_heap::<_, _, H>(&mut vm.state, new_bound).is_err() {
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        };

        // The heap is always grown even when the index nonsensical.
        // TODO PLA-974 revert to not growing the heap on failure as soon as zk_evm is fixed
        if bigger_than_last_address(pointer) {
            let _ = vm.state.use_gas(u32::MAX);
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        }

        let heap = H::get_heap(&vm.state);
        let value = vm.state.heaps[heap].read_u256(address);
        Register1::set(args, &mut vm.state, value);

        if INCREMENT {
            Register2::set(args, &mut vm.state, pointer + 32);
        }
    })
}

fn store<T, W: World<T>, H, In, const INCREMENT: bool, const HOOKING_ENABLED: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus
where
    T: Tracer,
    H: HeapFromState,
    In: Source,
{
    full_boilerplate::<H::Write, _, _>(vm, world, tracer, |vm, args, _, _| {
        // Pointers need not be masked here even though we do not care about them being pointers.
        // They will panic, though because they are larger than 2^32.
        let (pointer, _) = In::get_with_pointer_flag(args, &mut vm.state);

        let address = pointer.low_u32();
        let value = Register2::get(args, &mut vm.state);

        let new_bound = address.wrapping_add(32);
        if grow_heap::<_, _, H>(&mut vm.state, new_bound).is_err() {
            vm.state.current_frame.pc = spontaneous_panic();
            return ExecutionStatus::Running;
        }

        // The heap is always grown even when the index nonsensical.
        // TODO PLA-974 revert to not growing the heap on failure as soon as zk_evm is fixed
        if bigger_than_last_address(pointer) {
            let _ = vm.state.use_gas(u32::MAX);
            vm.state.current_frame.pc = spontaneous_panic();
            return ExecutionStatus::Running;
        }

        let heap = H::get_heap(&vm.state);
        vm.state.heaps.write_u256(heap, address, value);

        if INCREMENT {
            Register1::set(args, &mut vm.state, pointer + 32);
        }

        if HOOKING_ENABLED && address == vm.settings.hook_address {
            ExecutionStatus::Stopped(ExecutionEnd::SuspendedOnHook(value.as_u32()))
        } else {
            ExecutionStatus::Running
        }
    })
}

/// Pays for more heap space. Doesn't acually grow the heap.
/// That distinction is necessary because the bootloader gets `u32::MAX` heap for free.
pub(crate) fn grow_heap<T, W, H: HeapFromState>(
    state: &mut State<T, W>,
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

fn load_pointer<T: Tracer, W: World<T>, const INCREMENT: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<opcodes::PointerRead, _, _>(vm, world, tracer, |vm, args| {
        let (input, input_is_pointer) = Register1::get_with_pointer_flag(args, &mut vm.state);
        if !input_is_pointer {
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        }
        let pointer = FatPointer::from(input);

        // Usually, we just read zeroes instead of out-of-bounds bytes
        // but if offset + 32 is not representable, we panic, even if we could've read some bytes.
        // This is not a bug, this is how it must work to be backwards compatible.
        if pointer.offset > LAST_ADDRESS {
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        };

        let start = pointer.start + pointer.offset.min(pointer.length);
        let end = start.saturating_add(32).min(pointer.start + pointer.length);

        let value = vm.state.heaps[pointer.memory_page].read_u256_partially(start..end);
        Register1::set(args, &mut vm.state, value);

        if INCREMENT {
            // This addition does not overflow because we checked that the offset is small enough above.
            Register2::set_fat_ptr(args, &mut vm.state, input + 32);
        }
    })
}

impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a [`HeapRead`](opcodes::HeapRead) instruction with the provided params.
    pub fn from_heap_read(
        src: RegisterOrImmediate,
        out: Register1,
        incremented_out: Option<Register2>,
        arguments: Arguments,
    ) -> Self {
        Self::from_read::<Heap>(src, out, incremented_out, arguments)
    }

    /// Creates an [`AuxHeapRead`](opcodes::AuxHeapRead) instruction with the provided params.
    pub fn from_aux_heap_read(
        src: RegisterOrImmediate,
        out: Register1,
        incremented_out: Option<Register2>,
        arguments: Arguments,
    ) -> Self {
        Self::from_read::<AuxHeap>(src, out, incremented_out, arguments)
    }

    fn from_read<H: HeapFromState>(
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
            handler: monomorphize!(load [T W H] match_reg_imm src match_boolean increment),
            arguments,
        }
    }

    /// Creates a [`HeapWrite`](opcodes::HeapWrite) instruction with the provided params.
    pub fn from_heap_write(
        src1: RegisterOrImmediate,
        src2: Register2,
        incremented_out: Option<Register1>,
        arguments: Arguments,
        should_hook: bool,
    ) -> Self {
        Self::from_write::<Heap>(src1, src2, incremented_out, arguments, should_hook)
    }

    /// Creates an [`AuxHeapWrite`](opcodes::AuxHeapWrite) instruction with the provided params.
    pub fn from_aux_heap_store(
        src1: RegisterOrImmediate,
        src2: Register2,
        incremented_out: Option<Register1>,
        arguments: Arguments,
    ) -> Self {
        Self::from_write::<AuxHeap>(src1, src2, incremented_out, arguments, false)
    }

    fn from_write<H: HeapFromState>(
        src1: RegisterOrImmediate,
        src2: Register2,
        incremented_out: Option<Register1>,
        arguments: Arguments,
        should_hook: bool,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(store [T W H] match_reg_imm src1 match_boolean increment match_boolean should_hook),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_destination(&incremented_out),
        }
    }

    /// Creates an [`PointerRead`](opcodes::PointerRead) instruction with the provided params.
    pub fn from_pointer_read(
        src: Register1,
        out: Register1,
        incremented_out: Option<Register2>,
        arguments: Arguments,
    ) -> Self {
        let increment = incremented_out.is_some();
        Self {
            handler: monomorphize!(load_pointer [T W] match_boolean increment),
            arguments: arguments
                .write_source(&src)
                .write_destination(&out)
                .write_destination(&incremented_out),
        }
    }
}
