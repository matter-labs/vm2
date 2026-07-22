//! Covers the wiring between `push_frame`/`pop_frame` and `Heaps::live_logical_bytes`: every
//! non-kernel frame's stipend heaps (heap + aux heap) must be counted while the frame is live,
//! and kernel frames (the bootloader/system contracts, which get huge free heaps) must never
//! inflate the counter — it is a per-user-tx ceiling, not a kernel one.

use zkevm_opcode_defs::{
    ethereum_types::Address,
    system_params::{NEW_FRAME_MEMORY_STIPEND, NEW_KERNEL_FRAME_MEMORY_STIPEND},
};
use zksync_vm2_interface::opcodes;

use crate::{
    addressing_modes::{Arguments, Register, Register1},
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
    let program = Program::from_raw(vec![ret_instruction()], vec![]);
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
