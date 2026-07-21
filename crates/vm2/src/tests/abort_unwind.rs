//! Regression test for the uncatchable tx-wide unwind triggered by `State::abort_transaction`.
//!
//! Builds a four-deep call stack (bootloader -> A -> B -> C), each far call registering its own
//! exception handler in the caller, then arms the abort while paused deep in C. The unwind must
//! re-panic through every intermediate frame *without* running any of their exception handlers,
//! stopping only once control returns to the bootloader (frame 0).

use primitive_types::U256;
use zkevm_opcode_defs::ethereum_types::Address;
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

const GAS_TO_PASS: u64 = 10_000;
/// Index, within every `caller_program`, of the instruction reached if (and only if) its far
/// call's exception handler actually runs. Shared across bootloader/A/B for convenience; the
/// tracer disambiguates by also checking the frame's address.
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

fn ret_normal<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// A distinctive marker instruction for "this frame's exception handler ran". Distinct from the
/// `Ret::Panic` opcode used by the abort cascade itself, so if it ever executes we know it wasn't
/// the cascade's own spontaneous panic.
fn catch_marker<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_revert(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// Builds a program that loads `(abi, target_address)` from its code page (index 0 and 1
/// respectively), far-calls `target` with `CATCH_PC` as the exception handler, and — only if
/// that handler is reached — executes the `catch_marker` instruction at `CATCH_PC`.
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

/// Stops the first `run()` once the VM is four far frames deep (bootloader, A, B, C); on any
/// subsequent `run()`, also watches for the cascade delivering its final, normal panic into the
/// bootloader, and records whether A's exception handler ever ran.
struct AbortCascadeTracer {
    target_depth: usize,
    depth_reached: bool,
    a_address: Address,
    a_handler_ran: bool,
    bootloader_address: Address,
    bootloader_delivery_seen: bool,
}

impl AbortCascadeTracer {
    fn new(target_depth: usize, a_address: Address, bootloader_address: Address) -> Self {
        Self {
            target_depth,
            depth_reached: false,
            a_address,
            a_handler_ran: false,
            bootloader_address,
            bootloader_delivery_seen: false,
        }
    }
}

impl Tracer for AbortCascadeTracer {
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
        if !self.depth_reached && state.number_of_callframes() == self.target_depth {
            self.depth_reached = true;
            return ShouldStop::Stop;
        }

        let frame = state.current_frame();
        let is_bootloader = frame.address() == self.bootloader_address;
        let pc = frame.program_counter();
        drop(frame);
        // `flags().less_than` is how `Ret<Panic>` signals panic (see `naked_ret`'s
        // `Flags::new(return_type == ReturnType::Panic, false, false)`).
        let panicking = state.flags().less_than;
        if is_bootloader && pc == Some(CATCH_PC) && panicking {
            self.bootloader_delivery_seen = true;
            return ShouldStop::Stop;
        }
        ShouldStop::Continue
    }
}

#[test]
fn abort_unwinds_past_exception_handlers_to_bootloader() {
    let bootloader_address = Address::from_low_u64_be(0x_1000_0000_0000_0001);
    let a_address = Address::from_low_u64_be(0x_2000_0000_0000_0001);
    let b_address = Address::from_low_u64_be(0x_3000_0000_0000_0001);
    let c_address = Address::from_low_u64_be(0x_4000_0000_0000_0001);

    let bootloader_program: Program<AbortCascadeTracer, TestWorld<AbortCascadeTracer>> =
        caller_program(a_address, GAS_TO_PASS);
    let a_program: Program<AbortCascadeTracer, TestWorld<AbortCascadeTracer>> =
        caller_program(b_address, GAS_TO_PASS);
    let b_program: Program<AbortCascadeTracer, TestWorld<AbortCascadeTracer>> =
        caller_program(c_address, GAS_TO_PASS);
    let c_program: Program<AbortCascadeTracer, TestWorld<AbortCascadeTracer>> =
        Program::from_raw(vec![ret_normal()], vec![]);

    let mut world = TestWorld::new(&[
        (bootloader_address, bootloader_program),
        (a_address, a_program),
        (b_address, b_program),
        (c_address, c_program),
    ]);
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
        },
    );

    // bootloader + A + B + C, no near calls => 4 far frames.
    let mut tracer = AbortCascadeTracer::new(4, a_address, bootloader_address);

    let end = vm.run(&mut world, &mut tracer);
    assert_eq!(end, ExecutionEnd::StoppedByTracer);
    assert!(tracer.depth_reached, "tracer never observed depth 4");
    assert_eq!(
        vm.state.previous_frames.len(),
        3,
        "should be paused deep in C, with bootloader/A/B suspended below it"
    );

    // Arm the abort as if C hit an invariant violation.
    vm.state.abort_transaction();

    // Resume: the cascade fires.
    let end = vm.run(&mut world, &mut tracer);
    assert_eq!(end, ExecutionEnd::StoppedByTracer);
    assert!(
        tracer.bootloader_delivery_seen,
        "the panic never reached the bootloader"
    );

    // Uncatchability: despite A's exception handler being registered for B's failure, control
    // unwound all the way back to frame 0 without ever stopping at it. If any intermediate
    // handler had caught the panic, `previous_frames` would be non-empty here.
    assert!(vm.state.previous_frames.is_empty());
    assert!(!vm.state.aborting, "aborting flag must be cleared");
    assert!(
        !tracer.a_handler_ran,
        "A's exception handler must not have run — the unwind is supposed to skip it"
    );
    assert!(
        vm.flags().less_than,
        "the panic must be delivered to the bootloader"
    );
}
