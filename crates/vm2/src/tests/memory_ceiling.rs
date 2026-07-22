//! Enforcement tests for `Settings.memory_ceiling_bytes` (Task 6).
//!
//! Two production trigger sites can fire the uncatchable tx-wide abort when a *user* (non-kernel)
//! transaction would push the summed logical heap bytes over the configured ceiling:
//!
//! * `grow_heap` — a heap store whose growth would cross the ceiling (`grow_over_ceiling_*`);
//! * `far_call` — pushing the next frame, whose stipends would cross the ceiling
//!   (`far_call_stipend_over_ceiling_*`).
//!
//! Both build a bootloader -> A -> B call stack (A registers its own exception handler for B's
//! failure) with a LOW ceiling, drive the crossing, and assert the whole transaction unwound to
//! the bootloader (frame 0) *without* running A's exception handler — i.e. the ceiling breach is
//! uncatchable, exactly like the abort primitive in `abort_unwind.rs`. Both stipends are
//! `NEW_FRAME_MEMORY_STIPEND` (non-EVM, non-kernel), so each pushed frame counts `2 * STIP`.

use primitive_types::U256;
use zkevm_opcode_defs::{ethereum_types::Address, system_params::NEW_FRAME_MEMORY_STIPEND};
use zksync_vm2_interface::{
    opcodes, CallframeInterface, GlobalStateInterface, OpcodeType, ShouldStop, StateInterface,
    Tracer,
};

use crate::{
    addressing_modes::{
        Arguments, CodePage, Immediate1, Register, Register1, Register2, RegisterAndImmediate,
    },
    testonly::{initial_decommit, TestWorld},
    ExecutionEnd, Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
    World,
};

const GAS_TO_PASS: u64 = 50_000;
/// Index, within every `caller_program`, of the instruction reached only if that frame's far-call
/// exception handler actually runs — i.e. only if the panic were *catchable*.
const CATCH_PC: u16 = 3;
/// Non-kernel frame heap stipend. Each pushed non-kernel frame counts `2 * STIP` (heap + aux heap)
/// against `live_logical_bytes`.
const STIP: u64 = NEW_FRAME_MEMORY_STIPEND as u64;
/// Low ceiling shared by both tests. It admits bootloader -> A -> B (root frame is never counted;
/// A and B each add `2 * STIP`, reaching `4 * STIP`), but rejects the next crossing:
/// * grow test — B's heap growth (delta far exceeds `STIP`) pushes well past `5 * STIP`;
/// * far-call test — pushing C would reach `6 * STIP > 5 * STIP`.
const CEILING: u64 = 5 * STIP;

fn far_call_abi(gas: u64) -> U256 {
    let mut abi = U256::zero();
    abi.0[3] = gas;
    abi
}

fn address_word(address: Address) -> U256 {
    address.to_low_u64_be().into()
}

fn load_codepage<T: Tracer, W: World<T>>(immediate: u16, dest: Register) -> Instruction<T, W> {
    let r0 = Register::new(0);
    Instruction::from_add(
        CodePage(RegisterAndImmediate {
            immediate,
            register: r0,
        })
        .into(),
        Register2(r0),
        Register1(dest).into(),
        Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
        false,
        false,
    )
}

fn far_call_to<T: Tracer, W: World<T>>(exception_handler: u16) -> Instruction<T, W> {
    Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(exception_handler),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    )
}

fn ret_normal<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// Marker instruction for "this frame's exception handler ran". Distinct from the `Ret::Panic`
/// opcode that drives the abort cascade, so if it ever executes we know a handler actually caught.
fn catch_marker<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_revert(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// Loads `(abi, target_address)` from its code page, far-calls `target` with `CATCH_PC` as the
/// exception handler, and — only if that handler is reached — executes `catch_marker` at `CATCH_PC`.
fn caller_program<T: Tracer, W: World<T>>(target: Address, gas_to_pass: u64) -> Program<T, W> {
    Program::from_raw(
        vec![
            load_codepage(0, Register::new(1)),
            load_codepage(1, Register::new(2)),
            far_call_to(CATCH_PC),
            catch_marker(),
        ],
        vec![far_call_abi(gas_to_pass), address_word(target)],
    )
}

/// Leaf program that stores to a heap offset far past the free stipend, forcing `grow_heap` to
/// grow (and thus consult the ceiling). Loads the offset from its code page into r3 first, since a
/// real far call resets registers.
fn heap_grow_program<T: Tracer, W: World<T>>(offset: u32) -> Program<T, W> {
    let r3 = Register::new(3);
    Program::from_raw(
        vec![
            load_codepage(0, r3),
            Instruction::from_heap_write(
                Register1(r3).into(),
                Register2(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ),
            ret_normal(),
        ],
        vec![U256::from(offset)],
    )
}

/// Records whether A's exception handler ever ran, and stops the run the instant the panic is
/// delivered to the bootloader's exception handler — the terminal state of the tx-wide unwind.
struct CeilingTracer {
    a_address: Address,
    bootloader_address: Address,
    a_handler_ran: bool,
    bootloader_delivery_seen: bool,
}

impl Tracer for CeilingTracer {
    fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        let frame = state.current_frame();
        if frame.address() == self.a_address && frame.program_counter() == Some(CATCH_PC) {
            self.a_handler_ran = true;
        }
    }

    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(
        &mut self,
        state: &mut S,
    ) -> ShouldStop {
        let frame = state.current_frame();
        let is_bootloader = frame.address() == self.bootloader_address;
        let pc = frame.program_counter();
        drop(frame);
        // `flags().less_than` is how `Ret<Panic>` signals panic (see `naked_ret`).
        if is_bootloader && pc == Some(CATCH_PC) && state.flags().less_than {
            self.bootloader_delivery_seen = true;
            return ShouldStop::Stop;
        }
        ShouldStop::Continue
    }
}

/// Builds the VM over `[bootloader, A, B, (C)]` programs with the low `CEILING`, runs to the
/// tx-wide unwind, and asserts uncatchability. Shared by both trigger-site tests.
fn run_and_assert_uncatchable_revert(
    programs: &[(Address, Program<CeilingTracer, TestWorld<CeilingTracer>>)],
) {
    let bootloader_address = programs[0].0;
    let a_address = programs[1].0;

    let mut world = TestWorld::new(programs);
    let program = initial_decommit(&mut world, bootloader_address);

    let mut vm = VirtualMachine::new(
        bootloader_address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
            memory_ceiling_bytes: CEILING,
        },
    );

    let mut tracer = CeilingTracer {
        a_address,
        bootloader_address,
        a_handler_ran: false,
        bootloader_delivery_seen: false,
    };

    let end = vm.run(&mut world, &mut tracer);

    assert_eq!(
        end,
        ExecutionEnd::StoppedByTracer,
        "the run must stop at the panic's delivery to the bootloader"
    );
    assert!(
        tracer.bootloader_delivery_seen,
        "the panic never reached the bootloader — tx did not unwind to frame 0"
    );
    // Unwound all the way to frame 0: nothing left on the call stack.
    assert!(
        vm.state.previous_frames.is_empty(),
        "the whole tx must have unwound to the bootloader (frame 0)"
    );
    // The panic is delivered to the bootloader as a panic.
    assert!(
        vm.flags().less_than,
        "the bootloader must receive the delivery as a panic"
    );
    // Uncatchable: A's exception handler was registered for B's failure yet never ran.
    assert!(
        !tracer.a_handler_ran,
        "A's exception handler must be skipped — the ceiling breach is uncatchable"
    );
    // The abort flag is consumed by the terminal step of the unwind.
    assert!(
        !vm.state.aborting,
        "aborting must be cleared after the unwind"
    );
}

#[test]
fn grow_over_ceiling_reverts_whole_tx() {
    let bootloader_address = Address::from_low_u64_be(0x_1000_0000_0000_0001);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0001);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0001);

    // B grows its heap to ~100 KiB, a delta far larger than the whole `CEILING`, so the growth —
    // not the frame push — is what trips the check. The ceiling check runs *before* gas payment,
    // so B needing far more gas than it holds to actually pay for the growth is irrelevant.
    let programs = vec![
        (bootloader_address, caller_program(a_address, GAS_TO_PASS)),
        (a_address, caller_program(b_address, GAS_TO_PASS)),
        (b_address, heap_grow_program(100_000)),
    ];

    run_and_assert_uncatchable_revert(&programs);
}

#[test]
fn far_call_stipend_over_ceiling_reverts_whole_tx() {
    let bootloader_address = Address::from_low_u64_be(0x_1000_0000_0000_0001);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0001);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0001);
    let c_address = Address::from_low_u64_be(0x_4000_0000_0000_0001);

    // bootloader -> A -> B all push fine (reaching 4 * STIP); B's far-call to C would push the
    // running total to 6 * STIP > CEILING, so the far_call trigger aborts before pushing C.
    let programs = vec![
        (bootloader_address, caller_program(a_address, GAS_TO_PASS)),
        (a_address, caller_program(b_address, GAS_TO_PASS)),
        (b_address, caller_program(c_address, GAS_TO_PASS)),
        (c_address, Program::from_raw(vec![ret_normal()], vec![])),
    ];

    run_and_assert_uncatchable_revert(&programs);
}

/// Tracks the maximum call depth reached and stops when the panic is delivered to the bootloader.
/// A ceiling-breaching `far_call` must still PUSH a (panicking) frame — so `max_depth` reaches the
/// full nesting — then unwind completely. A regression that returned without pushing would leave
/// the `FarCall` opcode unbalanced against its `Ret` (a phantom open call-tree node) and top
/// `max_depth` out one short.
struct DepthBalanceTracer {
    bootloader_address: Address,
    max_depth: usize,
    bootloader_delivery_seen: bool,
}

impl Tracer for DepthBalanceTracer {
    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(
        &mut self,
        state: &mut S,
    ) -> ShouldStop {
        self.max_depth = self.max_depth.max(state.number_of_callframes());
        let frame = state.current_frame();
        let is_bootloader = frame.address() == self.bootloader_address;
        let pc = frame.program_counter();
        drop(frame);
        if is_bootloader && pc == Some(CATCH_PC) && state.flags().less_than {
            self.bootloader_delivery_seen = true;
            return ShouldStop::Stop;
        }
        ShouldStop::Continue
    }
}

#[test]
fn far_call_ceiling_abort_keeps_call_tree_balanced() {
    let bootloader_address = Address::from_low_u64_be(0x_1000_0000_0000_0001);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0001);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0001);
    let c_address = Address::from_low_u64_be(0x_4000_0000_0000_0001);

    let programs: Vec<(
        Address,
        Program<DepthBalanceTracer, TestWorld<DepthBalanceTracer>>,
    )> = vec![
        (bootloader_address, caller_program(a_address, GAS_TO_PASS)),
        (a_address, caller_program(b_address, GAS_TO_PASS)),
        (b_address, caller_program(c_address, GAS_TO_PASS)),
        (c_address, Program::from_raw(vec![ret_normal()], vec![])),
    ];

    let mut world = TestWorld::new(&programs);
    let program = initial_decommit(&mut world, bootloader_address);
    let mut vm = VirtualMachine::new(
        bootloader_address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
            memory_ceiling_bytes: CEILING,
        },
    );

    let mut tracer = DepthBalanceTracer {
        bootloader_address,
        max_depth: 0,
        bootloader_delivery_seen: false,
    };

    let end = vm.run(&mut world, &mut tracer);
    assert_eq!(end, ExecutionEnd::StoppedByTracer);
    assert!(
        tracer.bootloader_delivery_seen,
        "tx never unwound to the bootloader"
    );
    // The breaching far_call (B -> C) pushed a real (panicking) frame, so depth reached the full
    // four (bootloader, A, B, C). A no-push regression would top out at three, leaving the FarCall
    // opcode's traced callee node unbalanced against any Ret.
    assert_eq!(
        tracer.max_depth, 4,
        "the ceiling-breaching far_call must still push a frame (balanced FarCall<->Ret)"
    );
    // ...and it unwound the whole way: back at frame 0 only.
    assert!(
        vm.state.previous_frames.is_empty(),
        "must unwind to the bootloader"
    );
}

/// A minimal single-(root-)frame VM with the ceiling disabled, for the `State`-equality guards.
fn single_frame_vm() -> VirtualMachine<(), TestWorld<()>> {
    let address = Address::from_low_u64_be(0x_2000_0000_0000_0001);
    let program: Program<(), TestWorld<()>> = Program::from_raw(vec![ret_normal()], vec![]);
    let mut world = TestWorld::new(&[(address, program)]);
    let program = initial_decommit(&mut world, address);
    VirtualMachine::new(
        address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
            memory_ceiling_bytes: u64::MAX,
        },
    )
}

#[test]
fn transient_and_config_state_fields_excluded_from_eq() {
    // Guards the consensus-invisibility invariant: `aborting` (transient) and `memory_ceiling_bytes`
    // (config) must never leak into `State::eq`. Two states differing only in one of them compare
    // equal; a future edit adding either to equality fails this test loudly. (The heap counter's
    // exclusion is guarded in `heap.rs`'s `live_logical_bytes_excluded_from_heaps_eq`, since that
    // field is private to the heap module.)
    let vm = single_frame_vm();
    let base = vm.state.clone();

    let mut flipped = base.clone();
    flipped.aborting = !flipped.aborting;
    assert_eq!(
        base, flipped,
        "`aborting` must stay excluded from State::eq"
    );

    let mut reconfigured = base.clone();
    reconfigured.memory_ceiling_bytes ^= 0xdead_beef;
    assert_eq!(
        base, reconfigured,
        "`memory_ceiling_bytes` must stay excluded from State::eq"
    );
}

#[test]
fn excluded_state_fields_survive_snapshot_rollback() {
    // A snapshot -> mutate-excluded-fields -> rollback round-trip must land on an equal state, even
    // though rollback neither snapshots nor restores these fields (it resets `aborting` to false and
    // leaves `memory_ceiling_bytes` as-is): they are excluded from equality, so the round-trip is
    // consensus-invisible either way.
    let mut vm = single_frame_vm();
    vm.make_snapshot();
    let before = vm.state.clone();

    vm.state.aborting = true;
    vm.state.memory_ceiling_bytes ^= 0xbeef;
    vm.rollback();

    assert_eq!(
        before, vm.state,
        "excluded fields must not perturb a snapshot round-trip"
    );
}
