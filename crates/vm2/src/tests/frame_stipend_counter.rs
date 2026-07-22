//! Covers the wiring between `push_frame`/`pop_frame` and `Heaps::live_logical_bytes`: every
//! non-kernel frame's stipend heaps (heap + aux heap) must be counted while the frame is live,
//! and kernel frames (the bootloader/system contracts, which get huge free heaps) must never
//! inflate the counter — it is a per-user-tx ceiling, not a kernel one.

use primitive_types::U256;
use zkevm_opcode_defs::{
    ethereum_types::Address,
    system_params::{NEW_FRAME_MEMORY_STIPEND, NEW_KERNEL_FRAME_MEMORY_STIPEND},
};
use zksync_vm2_interface::opcodes;

use crate::{
    addressing_modes::{Arguments, Register, Register1, Register2},
    testonly::TestWorld,
    Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
};

fn default_settings() -> Settings {
    Settings {
        default_aa_code_hash: [0; 32],
        evm_interpreter_code_hash: [0; 32],
        hook_address: 0,
    }
}

/// First 18 bytes are zero, so this address executes in kernel mode (see `decommit::is_kernel`).
fn kernel_address() -> Address {
    Address::from_low_u64_be(1)
}

fn non_kernel_address() -> Address {
    Address::repeat_byte(1)
}

fn ret_instruction() -> Instruction<(), TestWorld<()>> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

fn new_vm(root_address: Address) -> VirtualMachine<(), TestWorld<()>> {
    let program = Program::from_raw(vec![ret_instruction()], vec![]);
    VirtualMachine::new(
        root_address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    )
}

/// Directly drives `push_frame`/`pop_frame` (bypassing the far-call/ret opcode ABI machinery,
/// which has its own returndata-forwarding rules irrelevant here) to isolate the counter wiring
/// under test. This mirrors the pattern already used by `divergence_regressions.rs` for
/// decommit-page tests.
fn push_child_frame(vm: &mut VirtualMachine<(), TestWorld<()>>, child_address: Address) {
    push_child_frame_with_program(
        vm,
        child_address,
        Program::from_raw(vec![ret_instruction()], vec![]),
    );
}

/// Like [`push_child_frame`], but lets the caller supply the child's program directly, so tests
/// can drive real instruction handlers (e.g. a heap store, to exercise `grow_heap`) via
/// [`execute_one_instruction`] instead of only poking `push_frame`/`pop_frame` state directly.
fn push_child_frame_with_program(
    vm: &mut VirtualMachine<(), TestWorld<()>>,
    child_address: Address,
    program: Program<(), TestWorld<()>>,
) {
    let calldata_heap = vm.state.current_frame.calldata_heap;
    let snapshot = vm.world_diff.snapshot();
    vm.push_frame::<opcodes::Normal>(
        child_address,
        program,
        200_000,
        0,
        false,
        false,
        calldata_heap,
        snapshot,
    );
}

/// Executes exactly the one instruction at the current frame's program counter, without looping
/// (unlike `VirtualMachine::run`, which runs until something stops it). Mirrors the identically
/// named helper in `divergence_regressions.rs`, specialized to this file's concrete `T`/`W`.
fn execute_one_instruction(vm: &mut VirtualMachine<(), TestWorld<()>>, world: &mut TestWorld<()>) {
    unsafe {
        let _ = ((*vm.state.current_frame.pc).handler)(vm, world, &mut ());
    }
}

#[test]
fn push_pop_moves_counter_by_stipend_for_user_frames() {
    let mut vm = new_vm(kernel_address());
    let base = vm.state.heaps.live_logical_bytes();
    assert_eq!(
        base, 0,
        "the root (kernel) frame's heaps must not be counted"
    );

    push_child_frame(&mut vm, non_kernel_address());

    let stip = u64::from(NEW_FRAME_MEMORY_STIPEND);
    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base + 2 * stip,
        "pushing a non-kernel frame must count both its heap and aux heap stipends"
    );

    vm.pop_frame(None).expect("child frame must be present");
    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base,
        "popping with nothing kept alive must return the counter to baseline"
    );
}

#[test]
fn kernel_frame_not_counted() {
    let mut vm = new_vm(kernel_address());
    let base = vm.state.heaps.live_logical_bytes();

    push_child_frame(&mut vm, kernel_address());

    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base,
        "kernel frames must never inflate the per-user-tx counter"
    );

    // Sanity check: this really did allocate a kernel-sized stipend that we're deliberately not
    // counting, not just a coincidentally-equal value.
    assert_eq!(
        vm.state.current_frame.heap_size,
        NEW_KERNEL_FRAME_MEMORY_STIPEND
    );
    assert!(vm.state.current_frame.is_kernel);

    vm.pop_frame(None).expect("child frame must be present");
    assert_eq!(vm.state.heaps.live_logical_bytes(), base);
}

#[test]
fn kept_returndata_heap_stays_counted_after_pop() {
    // A frame that keeps one of its own heaps alive (e.g. as returndata bubbled to the caller)
    // must not have that heap's stipend decremented on pop — only the discarded one should drop.
    let mut vm = new_vm(kernel_address());
    let base = vm.state.heaps.live_logical_bytes();

    push_child_frame(&mut vm, non_kernel_address());
    let stip = u64::from(NEW_FRAME_MEMORY_STIPEND);
    assert_eq!(vm.state.heaps.live_logical_bytes(), base + 2 * stip);

    let kept_heap = vm.state.current_frame.heap;
    vm.pop_frame(Some(kept_heap))
        .expect("child frame must be present");

    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base + stip,
        "the kept heap's stipend must remain counted; only the aux heap's is dropped"
    );
}

#[test]
fn grow_heap_raises_counter_and_reverts() {
    // A store far past the stipend forces `grow_heap` to actually grow the heap (rather than
    // being absorbed by the free stipend), which must raise `live_logical_bytes` by exactly the
    // growth over the stipend.
    const OFFSET: u32 = 100_000;
    const NEW_BOUND: u32 = OFFSET + 32; // 100_032

    const ADDR_REG: u8 = 2;
    const VAL_REG: u8 = 3;
    let heap_write = Instruction::from_heap_write(
        Register1(Register::new(ADDR_REG)).into(),
        Register2(Register::new(VAL_REG)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        false,
    );
    let program = Program::from_raw(vec![heap_write], vec![]);

    let mut vm = new_vm(kernel_address());
    let mut world = TestWorld::new(&[]);
    let base = vm.state.heaps.live_logical_bytes();

    push_child_frame_with_program(&mut vm, non_kernel_address(), program);

    let stip = u64::from(NEW_FRAME_MEMORY_STIPEND);
    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base + 2 * stip,
        "sanity: pushing the frame counted both stipends before any growth"
    );

    // Clear both registers' pointer-tag bits (registers persist across our direct `push_frame`,
    // which — unlike a real far call — does not reset them) so the store's address/value reads
    // are treated as plain integers.
    vm.state.register_pointer_flags &= !((1 << ADDR_REG) | (1 << VAL_REG));
    vm.state.registers[usize::from(ADDR_REG)] = U256::from(OFFSET);
    vm.state.registers[usize::from(VAL_REG)] = U256::from(0x1234_u64);

    execute_one_instruction(&mut vm, &mut world);

    assert_eq!(
        vm.state.current_frame.heap_size, NEW_BOUND,
        "the store must have grown the heap to cover its address"
    );
    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base + stip + u64::from(NEW_BOUND),
        "growth must raise the counter by the growth delta over the stipend \
         (aux heap stays at its stipend, untouched)"
    );

    // Revert the frame (via a direct `pop_frame(None)`, exactly like the other tests in this
    // file, rather than a real `Ret<Revert>` instruction — the return-ABI/fat-pointer forwarding
    // machinery is irrelevant to what's under test here). Task 3/4's `pop_frame`/`deallocate`
    // must restore the counter to baseline with no new code in `grow_heap` for this half.
    vm.pop_frame(None).expect("child frame must be present");
    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base,
        "reverting (popping) the frame must restore the counter to baseline"
    );
}
