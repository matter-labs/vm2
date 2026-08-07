//! Differential tests against `zk_evm`'s reference memory implementation.
//!
//! The rest of the suite asserts vm2's behaviour and argues about `zk_evm`'s in prose. These tests
//! measure it: they run vm2's real heap against `zk_evm`'s real `SimpleMemory`.
//!
//! # Scope, and what it does not cover
//!
//! These compare the **memory effect of a far return**, not whole-VM equivalence. That is the
//! mechanism the returndata-window compaction changes, and it is reachable exactly:
//! `zk_evm`'s far `ret` performs a single memory operation,
//!
//! ```text
//! // zk_evm/src/opcodes/execution/ret.rs:199-206
//! if finished_callstack.is_local_frame == false {
//!     vm_state.memory.finish_global_frame(
//!         finished_callstack.base_memory_page,
//!         finished_callstack.this_address,
//!         returndata_fat_pointer,
//!         Timestamp(..),
//!     );
//! ```
//!
//! so [`zk_evm_far_return`] calls that with the same arguments. `SimpleMemory`'s implementation of
//! it keeps every page — the clearing loop is present but commented out, above the note "we should
//! not clean pages" — which is the concrete form of "`zk_evm` never frees a memory page".
//!
//! What is assumed rather than measured: that `finish_global_frame` is the only memory mutation a
//! far return performs. Verified by reading the handler, not by executing it. Executing `zk_evm`'s
//! `ret` opcode would need a full `VmState` with all its oracles and a raw encoding of the
//! instruction, which the real [`Program`](crate::Program) does not carry.

use primitive_types::{H160, U256};
use zk_evm::{
    reference_impls::memory::SimpleMemory, zkevm_opcode_defs::FatPointer as ZkFatPointer,
};
use zk_evm_abstractions::{
    aux::{MemoryPage, Timestamp},
    vm::Memory,
};
use zksync_vm2_interface::{opcodes, HeapId};

use super::divergence_regressions::{
    execute_one_instruction, kernel_address, load_forward_ret_abi, ret_r1_instruction,
};
use crate::{
    heap::Heaps, page_ids::base_page_from_heap, testonly::TestWorld, FatPointer, Program, Settings,
    VirtualMachine,
};

/// 32-byte words mirrored out of a vm2 heap page. 8 KiB covers every offset these tests use.
const MIRRORED_WORDS: u32 = 256;

/// Byte offsets the scenarios assert on. All 32-byte aligned, so each maps to one `SimpleMemory`
/// slot: `IN_WINDOW` is inside the returned window, the other two are outside it and in different
/// 256-byte chunks, so compaction frees them.
const BELOW_WINDOW: u32 = 0;
const IN_WINDOW: u32 = 1024;
const ABOVE_WINDOW: u32 = 4096;

/// Copies `page` out of vm2's heaps into a fresh `SimpleMemory`, word for word, so both sides start
/// from the same memory.
fn mirror_page(heaps: &Heaps, page: HeapId) -> SimpleMemory {
    let words = (0..MIRRORED_WORDS)
        .map(|slot| heaps[page].read_u256(slot * 32))
        .collect();
    let mut memory = SimpleMemory::new_without_preallocations();
    memory.populate_page(vec![(page.as_u32(), words)]);
    memory
}

/// The one memory operation `zk_evm`'s far `ret` performs — see the module comment.
fn zk_evm_far_return(
    memory: &mut SimpleMemory,
    dying_frame_heap: HeapId,
    dying_frame_address: H160,
    returned: &FatPointer,
) {
    memory.finish_global_frame(
        MemoryPage(base_page_from_heap(dying_frame_heap)),
        dying_frame_address,
        ZkFatPointer {
            offset: returned.offset,
            memory_page: returned.memory_page.as_u32(),
            start: returned.start,
            length: returned.length,
        },
        Timestamp(0),
    );
}

/// Asserts vm2 and `zk_evm` hold the same word, naming both values on failure.
#[track_caller]
fn assert_word_eq(heaps: &Heaps, memory: &SimpleMemory, page: HeapId, offset: u32, what: &str) {
    let ours = heaps[page].read_u256(offset);
    let theirs =
        U256::from_big_endian(&memory.dump_full_page(page.as_u32())[(offset / 32) as usize]);
    assert_eq!(
        ours,
        theirs,
        "{what}: page {} offset {offset} — vm2 has {ours:#x}, zk_evm has {theirs:#x}",
        page.as_u32()
    );
}

fn kernel_vm() -> (VirtualMachine<(), TestWorld<()>>, TestWorld<()>) {
    let program: Program<(), TestWorld<()>> = Program::from_raw(vec![ret_r1_instruction()], vec![]);
    let vm = VirtualMachine::new(
        kernel_address(),
        program,
        H160::zero(),
        &[],
        1_000_000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );
    (vm, TestWorld::new(&[]))
}

/// Step 0: the mirroring itself. If vm2 and `zk_evm` disagree here, nothing built on top of
/// [`mirror_page`] means anything.
#[test]
fn mirroring_a_vm2_heap_into_zk_evm_memory_round_trips() {
    let (mut vm, _world) = kernel_vm();
    let page = vm.state.current_frame.heap;

    for (i, offset) in [BELOW_WINDOW, IN_WINDOW, ABOVE_WINDOW]
        .into_iter()
        .enumerate()
    {
        vm.state
            .heaps
            .write_u256(page, offset, U256::from(0xdead_beef_u64 + i as u64));
    }

    let memory = mirror_page(&vm.state.heaps, page);

    for offset in [BELOW_WINDOW, IN_WINDOW, ABOVE_WINDOW] {
        assert_word_eq(&vm.state.heaps, &memory, page, offset, "mirrored word");
    }
    // A word never written must agree too — absent in vm2, absent in zk_evm, both read as zero.
    assert_word_eq(&vm.state.heaps, &memory, page, 2048, "untouched word");
}

/// Step 3: the divergence this PR fixes, measured rather than argued.
///
/// `E (initial) -> A (victim, pushed so its heap is a dynamic page) -> B (kernel)`. `B` ret-forwards
/// its calldata pointer, which names `A`'s heap — a page the dying frame does not own. Before the
/// gate, `pop_frame` compacted it and `A` resumed reading zeros; `zk_evm` keeps the bytes.
///
/// The victim must be a pushed frame: the initial frame's heap is the bootloader page, which
/// `compact_to_window` skips as `is_always_allocated`, so the bug cannot show up there.
#[test]
fn kernel_ret_forward_matches_zk_evm_on_a_live_callers_heap() {
    let (mut vm, mut world) = kernel_vm();
    let program: Program<(), TestWorld<()>> = Program::from_raw(vec![ret_r1_instruction()], vec![]);

    // E -> A. A is the victim: still live when B returns.
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        program.clone(),
        400_000,
        0,
        false,
        false,
        vm.state.current_frame.calldata_heap,
        vm.world_diff.snapshot(),
    );
    let victim_heap = vm.state.current_frame.heap;
    for offset in [BELOW_WINDOW, IN_WINDOW, ABOVE_WINDOW] {
        vm.state
            .heaps
            .write_u256(victim_heap, offset, U256::from(0xdead_beef_u64));
    }

    // A -> B (kernel), with B's calldata naming A's heap.
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        program,
        200_000,
        0,
        false,
        false,
        victim_heap,
        vm.world_diff.snapshot(),
    );
    assert!(vm.state.current_frame.is_kernel, "B must be kernel");
    assert_eq!(vm.state.current_frame.calldata_heap, victim_heap);
    let dying_heap = vm.state.current_frame.heap;
    let dying_address = vm.state.current_frame.address;

    // Both sides start from the same memory.
    let mut zk_memory = mirror_page(&vm.state.heaps, victim_heap);

    // B forwards a 32-byte window of A's heap and returns.
    load_forward_ret_abi(&mut vm, victim_heap, IN_WINDOW, 32);
    execute_one_instruction(&mut vm, &mut world, &mut ());

    // Positive controls: the scenario really did what it claims, so it cannot pass vacuously.
    let returned = FatPointer::from(vm.state.registers[1]);
    assert_eq!(
        (returned.memory_page, returned.start, returned.length),
        (victim_heap, IN_WINDOW, 32),
        "B's forward must survive the kernel filter and name A's heap"
    );
    assert_eq!(
        vm.state.current_frame.heap, victim_heap,
        "must be back in the still-live victim frame"
    );

    zk_evm_far_return(&mut zk_memory, dying_heap, dying_address, &returned);

    assert_word_eq(
        &vm.state.heaps,
        &zk_memory,
        victim_heap,
        IN_WINDOW,
        "in-window byte",
    );
    assert_word_eq(
        &vm.state.heaps,
        &zk_memory,
        victim_heap,
        BELOW_WINDOW,
        "byte below the window, in a live frame's heap",
    );
    assert_word_eq(
        &vm.state.heaps,
        &zk_memory,
        victim_heap,
        ABOVE_WINDOW,
        "byte above the window, in a live frame's heap",
    );
}
