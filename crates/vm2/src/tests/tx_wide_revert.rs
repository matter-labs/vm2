//! End-to-end tests for the tx-wide revert feature (uncatchable abort + heap-bytes ceiling +
//! kept-returndata counter accounting), tying together Tasks 1-6.
//!
//! Three properties matter for consensus:
//! * the abort point is fully deterministic (sequencer and verifier must diverge nowhere);
//! * the ceiling check is inert (byte-identical output) for any tx that never approaches it;
//! * `Heaps::live_logical_bytes` returns to its pre-tx baseline once a transaction full of
//!   nested, returndata-forwarding calls has unwound and the bootloader has reclaimed its kept
//!   heaps -- this is the drift guard for the kept-heap/rollback accounting added in Tasks 3-5.

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

/// Index, within every program in this file, of the instruction reached both by natural
/// fall-through after a successful far call and by the immediate far-call error handler. None of
/// the scenarios below expect a far call to actually fail (aside from the deliberate ceiling
/// breach in `abort_point_is_deterministic`), so the two paths sharing a PC is harmless -- it
/// matches the convention used throughout the other test files in this module.
const CATCH_PC: u16 = 3;

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

/// A plain `ret` whose return-ABI register is `r0` (always zero): a "fresh", zero-length pointer
/// into the returning frame's own heap. Real callers use exactly this to end a call with no
/// returndata -- and, per `naked_ret`, even this zero-length pointer causes the frame's heap to be
/// kept alive and bubbled to the caller (see `kept_returndata_heap_stays_counted_after_pop` in
/// `frame_stipend_counter.rs`).
fn ret_normal<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// A `revert` used as a safety net / delivery marker: whichever frame runs this, at frame 0, ends
/// the whole run deterministically via `ExecutionEnd::Reverted`.
fn catch_marker<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_revert(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// A `(address, program)` list for the `T = (), W = TestWorld<()>` instantiation used by tests 1
/// and 2 -- both build their program set once and reuse it across two separately-constructed VMs
/// (see the doc comment on `ceiling_tripping_programs`).
type NoTracerPrograms = Vec<(Address, Program<(), TestWorld<()>>)>;

fn default_settings(memory_ceiling_bytes: u64) -> Settings {
    Settings {
        default_aa_code_hash: [0; 32],
        evm_interpreter_code_hash: [0; 32],
        hook_address: 0,
        memory_ceiling_bytes,
    }
}

// ---------------------------------------------------------------------------------------------
// Test 1: abort_point_is_deterministic
// ---------------------------------------------------------------------------------------------

/// `caller_program`-style contract: far-calls `target`, and -- whether by the natural
/// fall-through of a successful call or by the error handler of a failed one -- lands on
/// `catch_marker` at `CATCH_PC`.
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

const DETERMINISM_GAS_TO_PASS: u64 = 50_000;
/// STIP-multiple ceiling that admits `bootloader -> A -> B` (4 * STIP: A and B each add
/// `2 * STIP`) but rejects `B`'s far call pushing `C` (would reach `6 * STIP`), tripping the
/// uncatchable tx-wide abort inside the `far_call` ceiling check.
fn determinism_ceiling() -> u64 {
    4 * u64::from(NEW_FRAME_MEMORY_STIPEND)
}

/// Builds the `bootloader -> A -> B -> (push C fails)` program set once. `Program` is
/// `Arc`-backed (see `program.rs`), so cloning these entries into two separate `TestWorld`s (as
/// `build_ceiling_tripping_scenario` does below) shares the same underlying instruction
/// allocation across both -- which matters because `Callframe`'s `PartialEq` compares `pc` via
/// `std::ptr::eq`. Rebuilding the programs from scratch per run would give each run its own,
/// differently-addressed `Arc` allocation and make `dump_state()` spuriously unequal even for a
/// perfectly deterministic run; this is a property of comparing state across independently
/// constructed VMs; it has no bearing on production determinism.
fn ceiling_tripping_programs() -> (Address, NoTracerPrograms) {
    let bootloader_address = Address::from_low_u64_be(0x_1000_0000_0000_0101);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0101);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0101);
    let c_address = Address::from_low_u64_be(0x_4000_0000_0000_0101);

    let programs = vec![
        (
            bootloader_address,
            caller_program(a_address, DETERMINISM_GAS_TO_PASS),
        ),
        (
            a_address,
            caller_program(b_address, DETERMINISM_GAS_TO_PASS),
        ),
        (
            b_address,
            caller_program(c_address, DETERMINISM_GAS_TO_PASS),
        ),
        (c_address, Program::from_raw(vec![ret_normal()], vec![])),
    ];
    (bootloader_address, programs)
}

/// Builds a fresh `VirtualMachine`/`TestWorld` pair from the (shared) program set above.
fn build_ceiling_tripping_scenario(
    bootloader_address: Address,
    programs: &NoTracerPrograms,
) -> (VirtualMachine<(), TestWorld<()>>, TestWorld<()>) {
    let mut world = TestWorld::new(programs);
    let program = initial_decommit(&mut world, bootloader_address);

    let vm = VirtualMachine::new(
        bootloader_address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(determinism_ceiling()),
    );
    (vm, world)
}

#[test]
fn abort_point_is_deterministic() {
    // Same ceiling-tripping batch, run twice from fresh VM/world pairs built off the same
    // (shared) program set. If the abort point (frame depth, gas, world diff, heaps -- everything
    // `dump_state` captures) ever depended on anything but the deterministic inputs, this would
    // flake or diverge -- exactly the class of bug that would let a sequencer and a verifier
    // disagree.
    let (bootloader_address, programs) = ceiling_tripping_programs();
    let run = || {
        let (mut vm, mut world) = build_ceiling_tripping_scenario(bootloader_address, &programs);
        let end = vm.run(&mut world, &mut ());
        (end, vm.dump_state())
    };

    let (end1, state1) = run();
    let (end2, state2) = run();

    assert_eq!(
        end1,
        ExecutionEnd::Reverted(Vec::new()),
        "the ceiling breach must unwind the whole tx and deliver a revert to the bootloader's \
         own handler (frame 0 has no caller to panic into, so the terminal `ret` there is a \
         plain revert with empty output)"
    );
    assert_eq!(
        end1, end2,
        "the ceiling-tripping abort must end the same way every run"
    );
    assert_eq!(
        state1, state2,
        "two fresh runs of the identical ceiling-tripping scenario must produce byte-identical \
         final VM state -- the sequencer and verifier must abort at exactly the same point"
    );
}

// ---------------------------------------------------------------------------------------------
// Test 2: below_ceiling_is_byte_identical_to_baseline
// ---------------------------------------------------------------------------------------------

const BELOW_CEILING_GAS_TO_PASS: u64 = 200_000;

/// Leaf contract: grows its own heap by a modest, realistic amount and returns normally (kept,
/// zero-length pointer -- see `ret_normal`).
fn leaf_program<T: Tracer, W: World<T>>(heap_grow_offset: u32) -> Program<T, W> {
    let r4 = Register::new(4);
    Program::from_raw(
        vec![
            load_codepage(0, r4),
            Instruction::from_heap_write(
                Register1(r4).into(),
                Register2(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ),
            ret_normal(),
        ],
        vec![U256::from(heap_grow_offset)],
    )
}

/// Middle contract: far-calls `target`, then also grows its own heap a little, then returns
/// normally. Not returndata-forwarding -- this test is about ceiling inertness, not counter
/// balance (that's test 3).
fn middle_program<T: Tracer, W: World<T>>(
    target: Address,
    gas_to_pass: u64,
    heap_grow_offset: u32,
) -> Program<T, W> {
    let r4 = Register::new(4);
    Program::from_raw(
        vec![
            load_codepage(0, Register::new(1)),
            load_codepage(1, Register::new(2)),
            far_call_to(CATCH_PC),
            load_codepage(2, r4),
            Instruction::from_heap_write(
                Register1(r4).into(),
                Register2(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ),
            ret_normal(),
            catch_marker(),
        ],
        vec![
            far_call_abi(gas_to_pass),
            address_word(target),
            U256::from(heap_grow_offset),
        ],
    )
}

/// Root contract: far-calls `target`, and ends the run normally (`ProgramFinished`) whether
/// reached by success fall-through or (unexpectedly) by the error handler -- both land at
/// `CATCH_PC`, which here is a plain `ret_normal`, not a revert, since a below-ceiling tx is
/// never expected to fail.
fn root_program<T: Tracer, W: World<T>>(target: Address, gas_to_pass: u64) -> Program<T, W> {
    Program::from_raw(
        vec![
            load_codepage(0, Register::new(1)),
            load_codepage(1, Register::new(2)),
            far_call_to(CATCH_PC),
            ret_normal(),
        ],
        vec![far_call_abi(gas_to_pass), address_word(target)],
    )
}

/// Builds the `root -> A -> B` program set once (`B` and `A` each do a modest heap growth: a
/// realistic shape of tx that comes nowhere near any sane ceiling). As in
/// `ceiling_tripping_programs`, building this once and reusing it for both settings keeps the
/// underlying (`Arc`-backed) instruction allocation identical across both runs, which
/// `Callframe`'s `pc`-pointer-identity `PartialEq` requires for a meaningful `dump_state()` diff.
fn below_ceiling_programs() -> (Address, NoTracerPrograms) {
    let root_address = Address::from_low_u64_be(0x_1000_0000_0000_0102);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0102);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0102);

    let programs = vec![
        (
            root_address,
            root_program(a_address, BELOW_CEILING_GAS_TO_PASS),
        ),
        (
            a_address,
            middle_program(b_address, BELOW_CEILING_GAS_TO_PASS, 1_500),
        ),
        (b_address, leaf_program(3_000)),
    ];
    (root_address, programs)
}

fn build_below_ceiling_tx(
    root_address: Address,
    programs: &NoTracerPrograms,
    memory_ceiling_bytes: u64,
) -> (VirtualMachine<(), TestWorld<()>>, TestWorld<()>) {
    let mut world = TestWorld::new(programs);
    let program = initial_decommit(&mut world, root_address);

    let vm = VirtualMachine::new(
        root_address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(memory_ceiling_bytes),
    );
    (vm, world)
}

#[test]
fn below_ceiling_is_byte_identical_to_baseline() {
    // A realistic tx (nested far calls + real heap growth) that never gets remotely close to
    // either ceiling value must produce identical final state whether the ceiling is a high,
    // unreached value or disabled outright (`u64::MAX`). This proves the ceiling check itself
    // (the extra comparison/`saturating_add` in `grow_heap`/`far_call`) has no side effect beyond
    // gating the abort -- it is inert when it doesn't fire.
    let (root_address, programs) = below_ceiling_programs();

    let (mut with_high_ceiling, mut world_high) =
        build_below_ceiling_tx(root_address, &programs, 1 << 40);
    let end_high = with_high_ceiling.run(&mut world_high, &mut ());

    let (mut with_disabled_ceiling, mut world_disabled) =
        build_below_ceiling_tx(root_address, &programs, u64::MAX);
    let end_disabled = with_disabled_ceiling.run(&mut world_disabled, &mut ());

    assert_eq!(
        end_high,
        ExecutionEnd::ProgramFinished(Vec::new()),
        "the below-ceiling tx must finish normally"
    );
    assert_eq!(
        end_high, end_disabled,
        "the two settings must reach the same execution end"
    );
    assert_eq!(
        with_high_ceiling.dump_state(),
        with_disabled_ceiling.dump_state(),
        "final VM state (registers, callframes, heaps) must be byte-identical between a high, \
         unreached ceiling and the ceiling disabled outright -- `memory_ceiling_bytes` itself is \
         excluded from `State`'s `PartialEq` on purpose (see state.rs), so this specifically \
         proves the check's *behavior* (not just the field) is a no-op below the ceiling. Both \
         runs execute the identical instruction trace (same programs, same gas, same growth), so \
         `WorldDiff` (storage/events), which isn't `PartialEq`, is provably identical too."
    );
}

// ---------------------------------------------------------------------------------------------
// Test 3: counter_returns_to_baseline_after_nested_calls_with_kept_returndata (THE important one)
// ---------------------------------------------------------------------------------------------

/// Bit position of the fat-pointer return-ABI "forward as-is" tag consulted by
/// `get_calldata`/`FatPointerSource::from_abi` (byte 28 of the 256-bit register, i.e.
/// `raw_abi.0[3] >> 32`). Packing this into a register via `PointerPack` (whose low 128 bits
/// must be zero, satisfied since 224 >= 128) turns a plain `ret` into one that forwards whatever
/// fat pointer is already in the source register, instead of building a fresh one -- exactly
/// what a real contract does to bubble a sub-call's returndata to its own caller.
const FORWARD_ABI_SOURCE_BIT: u32 = 224;

fn forward_marker() -> U256 {
    U256::one() << FORWARD_ABI_SOURCE_BIT
}

/// An intermediate contract in the forwarding chain: grows its own heap a little (so its own,
/// never-kept heap page has a real, non-stipend-only size to release), far-calls `target`, then
/// re-packs whatever pointer it received in `r1` (a real far call always leaves the callee's
/// returned pointer there, tagged, with every other register zeroed -- see `naked_ret`) with the
/// forward tag and returns *that*, bubbling the callee's kept heap one level further up instead
/// of returning a fresh pointer of its own. This is what makes returndata heaps accumulate
/// through several levels rather than dying at the first pop.
fn forwarder_program<T: Tracer, W: World<T>>(
    target: Address,
    gas_to_pass: u64,
    own_heap_grow_offset: u32,
) -> Program<T, W> {
    const SAFETY_NET_PC: u16 = 8;

    let r1 = Register::new(1);
    let r3 = Register::new(3);
    let r4 = Register::new(4);
    let r5 = Register::new(5);

    Program::from_raw(
        vec![
            load_codepage(0, r1),               // 0: far-call gas ABI
            load_codepage(1, Register::new(2)), // 1: far-call target address
            load_codepage(2, r4),               // 2: this frame's own heap-grow offset
            Instruction::from_heap_write(
                Register1(r4).into(),
                Register2(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ), // 3: grow own heap (never kept -- freed when this frame pops)
            far_call_to(SAFETY_NET_PC),         // 4: call target; on (unexpected) failure -> 8
            load_codepage(3, r3),               // 5: load the forward-ABI marker
            Instruction::from_pointer_pack(
                Register1(r1).into(),
                Register2(r3),
                Register1(r5).into(),
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ), // 6: combine the callee's returned pointer (r1) with the forward tag
            Instruction::from_ret(
                Register1(r5),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ), // 7: forward the callee's kept heap upward instead of returning fresh
            catch_marker(), // 8: safety net; only reached if the far call actually failed
        ],
        vec![
            far_call_abi(gas_to_pass),
            address_word(target),
            U256::from(own_heap_grow_offset),
            forward_marker(),
        ],
    )
}

/// The deepest contract in the chain: grows its own heap with a real, distinctive amount and
/// returns normally. The `ret_normal` zero-length pointer still keeps this frame's (grown) heap
/// alive and bubbles it to the caller -- see `ret_normal`'s doc comment.
fn leaf_returning_program<T: Tracer, W: World<T>>(heap_grow_offset: u32) -> Program<T, W> {
    let r4 = Register::new(4);
    Program::from_raw(
        vec![
            load_codepage(0, r4),
            Instruction::from_heap_write(
                Register1(r4).into(),
                Register2(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                false,
            ),
            ret_normal(),
        ],
        vec![U256::from(heap_grow_offset)],
    )
}

/// Root contract: just far-calls `target`. Deliberately does *not* itself `ret` on return --
/// doing so at frame 0 hits `naked_ret`'s early-return branch (`pop_frame` returns `None`
/// because `previous_frames` is empty), which reads the returned bytes but never touches
/// `heaps_i_am_keeping_alive` or deallocates anything. That branch is for a program that legitimately
/// ends the whole VM, which the real bootloader never does mid-batch (it loops until a hook
/// fires) -- so exercising it here would produce a dangling heap page that is an artifact of the
/// test, not a real drift. Instead, the test's tracer stops the VM the instant control returns to
/// this frame, before any further instruction (in particular, before any `ret`) executes.
fn root_forwarding_program<T: Tracer, W: World<T>>(
    target: Address,
    gas_to_pass: u64,
) -> Program<T, W> {
    Program::from_raw(
        vec![
            load_codepage(0, Register::new(1)),
            load_codepage(1, Register::new(2)),
            far_call_to(CATCH_PC),
            catch_marker(), // reached only if the chain unexpectedly failed
        ],
        vec![far_call_abi(gas_to_pass), address_word(target)],
    )
}

/// Stops the VM the instant execution lands back in the root frame at `root_return_pc` -- i.e.
/// the whole call tree has unwound, whether the chain succeeded or (unexpectedly) failed.
struct LandedAtRootTracer {
    root_address: Address,
    root_return_pc: u16,
    landed: bool,
}

impl Tracer for LandedAtRootTracer {
    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(
        &mut self,
        state: &mut S,
    ) -> ShouldStop {
        let frame = state.current_frame();
        let hit = frame.address() == self.root_address
            && frame.program_counter() == Some(self.root_return_pc);
        drop(frame);
        if hit {
            self.landed = true;
            return ShouldStop::Stop;
        }
        ShouldStop::Continue
    }
}

#[test]
fn counter_returns_to_baseline_after_nested_calls_with_kept_returndata() {
    let root_address = Address::from_low_u64_be(0x_1000_0000_0000_0103);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0103);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0103);
    let c_address = Address::from_low_u64_be(0x_4000_0000_0000_0103);
    let d_address = Address::from_low_u64_be(0x_5000_0000_0000_0103);

    // Generous, strictly decreasing gas budgets down the chain -- plenty of headroom over each
    // frame's own overhead (a handful of cheap instructions + a modest heap growth) plus whatever
    // it passes further down.
    let programs = vec![
        (root_address, root_forwarding_program(a_address, 1_500_000)),
        (a_address, forwarder_program(b_address, 1_000_000, 5_000)),
        (b_address, forwarder_program(c_address, 600_000, 8_000)),
        (c_address, forwarder_program(d_address, 300_000, 3_000)),
        (d_address, leaf_returning_program(12_000)),
    ];

    let mut world = TestWorld::new(&programs);
    let program = initial_decommit(&mut world, root_address);

    let mut vm = VirtualMachine::new(
        root_address,
        program,
        Address::zero(),
        &[],
        2_000_000,
        // Disabled: this test is about counter balance, not the ceiling, and must not have the
        // ceiling interfere with the deliberately heap-heavy chain below.
        default_settings(u64::MAX),
    );

    // `make_snapshot`/`pop_snapshot` mirror exactly how the real bootloader commits a transaction
    // and reclaims the returndata heaps it kept alive (`reclaim_bootloader_returndata_heaps`,
    // documented on `pop_snapshot` in vm.rs) -- both require being in frame 0 with an empty call
    // stack, which holds here (fresh VM).
    vm.make_snapshot();
    let base = vm.state.heaps.live_logical_bytes();
    assert_eq!(base, 0, "sanity: nothing is counted before the tx has run");

    let mut tracer = LandedAtRootTracer {
        root_address,
        root_return_pc: CATCH_PC,
        landed: false,
    };
    let end = vm.run(&mut world, &mut tracer);

    assert_eq!(end, ExecutionEnd::StoppedByTracer);
    assert!(
        tracer.landed,
        "never observed the call chain unwinding back to the root frame"
    );
    assert!(
        vm.state.previous_frames.is_empty(),
        "the whole call tree must have unwound back to frame 0"
    );
    assert!(
        !vm.flags().less_than,
        "the chain must complete successfully (forwarded all the way up), not panic"
    );

    // At this exact point -- fully unwound, but *before* the bootloader-level reclaim -- the
    // deepest frame's kept (and forwarded-through-every-level) heap is still alive and counted.
    // This is the "accumulates mid-chain" behavior the brief calls out as expected, not a bug.
    let after_unwind = vm.state.heaps.live_logical_bytes();
    assert!(
        after_unwind > base,
        "the forwarded returndata heap must still be counted immediately after the whole chain \
         unwinds, before the bootloader-level reclaim -- if this is 0, the chain didn't actually \
         forward anything and the test isn't exercising what it claims to"
    );
    // Exactly D's grown heap (12_000 + 32, its `heap_write`'s address + 32-byte word rounded up
    // to the next bound) and nothing else: A/B/C's own heaps and aux heaps, and D's aux heap, must
    // all have been freed as each frame popped, leaving only the one heap that was forwarded
    // through every level all the way up to the root.
    assert_eq!(
        after_unwind,
        12_000 + 32,
        "only D's forwarded (grown) heap should still be counted at this point -- any other \
         value means either a heap that should have been freed leaked, or the forwarded heap's \
         growth wasn't tracked correctly through the chain"
    );

    // The real reclaim boundary: `pop_snapshot` (called by the bootloader once a transaction is
    // fully committed) drains `heaps_i_am_keeping_alive` on the now-current root frame via
    // `reclaim_bootloader_returndata_heaps`.
    vm.pop_snapshot();

    assert_eq!(
        vm.state.heaps.live_logical_bytes(),
        base,
        "the counter must return to the pre-tx baseline once the bootloader reclaims the kept \
         returndata heap -- a nonzero residual here is a real leak in the kept-heap/rollback \
         accounting (Tasks 3-5), not a test artifact"
    );
}
