use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    decoding::EncodingModeProduction, ethereum_types::Address, system_params::VM_MAX_STACK_DEPTH,
    Condition, DecodedOpcode, ImmMemHandlerFlags, Opcode, Operand, RegOrImmFlags, UMAOpcode,
    OPCODES_TABLE, UMA_INCREMENT_FLAG_IDX,
};
use zksync_vm2_interface::{opcodes, Tracer};

use crate::{
    addressing_modes::{Arguments, Immediate1, Register, Register1, Register2},
    decode::decode,
    fat_pointer::FatPointer,
    testonly::TestWorld,
    ExecutionEnd, Instruction, ModeRequirements, Predicate, Program, Settings, StorageInterface,
    StorageSlot, VirtualMachine, World,
};

fn default_settings() -> Settings {
    Settings {
        default_aa_code_hash: [0; 32],
        evm_interpreter_code_hash: [0; 32],
        hook_address: 0,
    }
}

fn kernel_address() -> Address {
    // First 18 bytes are zero, so this address executes in kernel mode.
    Address::from_low_u64_be(1)
}

fn non_kernel_address() -> Address {
    Address::repeat_byte(1)
}

fn execute_one_instruction<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) {
    unsafe {
        let _ = ((*vm.state.current_frame.pc).handler)(vm, world, tracer);
    }
}

fn ret_instruction<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

fn static_uma_instruction<T: Tracer, W: World<T>>(opcode: UMAOpcode) -> Instruction<T, W> {
    let variant = OPCODES_TABLE
        .iter()
        .copied()
        .find(|variant| {
            variant.opcode == Opcode::UMA(opcode)
                && variant.src0_operand_type == Operand::RegOrImm(RegOrImmFlags::UseRegOnly)
                && matches!(
                    variant.dst0_operand_type,
                    Operand::RegOnly | Operand::Full(ImmMemHandlerFlags::UseRegOnly)
                )
                && !variant.flags[UMA_INCREMENT_FLAG_IDX]
        })
        .expect("Static UMA Register-only variant must exist");

    let encoded = DecodedOpcode::<8, EncodingModeProduction> {
        variant,
        condition: Condition::Always,
        src0_reg_idx: 0,
        src1_reg_idx: 0,
        dst0_reg_idx: 1,
        dst1_reg_idx: 0,
        imm_0: 0,
        imm_1: 0,
    }
    .serialize_as_integer();

    decode(encoded, false)
}

#[test]
fn static_memory_read_should_not_panic_in_kernel_mode() {
    // In zk_evm this opcode is executable in kernel mode. We lock this behavior as a regression
    // test before implementing StaticMemoryRead in vm2.
    let program = Program::from_raw(
        vec![
            static_uma_instruction(UMAOpcode::StaticMemoryRead),
            ret_instruction(),
        ],
        vec![],
    );
    let mut world = TestWorld::new(&[]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
}

#[test]
fn static_memory_write_should_not_panic_in_kernel_mode() {
    // In zk_evm this opcode is executable in kernel mode. We lock this behavior as a regression
    // test before implementing StaticMemoryWrite in vm2.
    let program = Program::from_raw(
        vec![
            static_uma_instruction(UMAOpcode::StaticMemoryWrite),
            ret_instruction(),
        ],
        vec![],
    );
    let mut world = TestWorld::new(&[]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
}

#[derive(Debug, Default)]
struct CountingWorld {
    storage_reads: usize,
}

impl StorageInterface for CountingWorld {
    fn read_storage(&mut self, _: H160, _: U256) -> StorageSlot {
        self.storage_reads += 1;
        StorageSlot::EMPTY
    }

    fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
        0
    }

    fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
        false
    }
}

impl<T: Tracer> World<T> for CountingWorld {
    fn decommit(&mut self, _: U256) -> Program<T, Self> {
        Program::new_panicking()
    }

    fn decommit_code(&mut self, _: U256) -> Vec<u8> {
        vec![]
    }
}

#[test]
fn shard_far_call_should_not_touch_storage_on_nonzero_shard() {
    // In zk_evm, non-zero shard calls fail before deployer storage lookups.
    let far_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1),
        false,
        true,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![far_call, ret_instruction()], vec![]);

    let mut world = CountingWorld::default();
    let mut vm = VirtualMachine::new(
        Address::from_low_u64_be(0x100),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // Use a plain ABI value (not a pointer), but make shard_id non-zero.
    vm.state.register_pointer_flags &= !(1 << 1);
    let mut abi = U256::zero();
    abi.0[3] = 1_u64 << 40;
    vm.state.registers[1] = abi;
    vm.state.registers[2] = U256::from(0x1234_u64);

    let _ = vm.run(&mut world, &mut ());

    assert_eq!(world.storage_reads, 0);
}

#[test]
fn precompile_extra_ergs_oog_should_not_panic() {
    // In zk_evm, PrecompileCall with insufficient extra ergs writes zero to dst and continues.
    // We intentionally follow the precompile call with two 0-cost instructions to verify that
    // execution continues to the next opcode instead of turning the current opcode into panic.
    let precompile_call = Instruction::from_precompile_call(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let add_zero_cost = Instruction::from_add(
        Register1(Register::new(0)).into(),
        Register2(Register::new(0)),
        Register1(Register::new(0)).into(),
        Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
        false,
        false,
    );
    let ret_zero_cost = Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 0, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![precompile_call, add_zero_cost, ret_zero_cost], vec![]);
    let mut world = TestWorld::new(&[]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = U256::zero();
    vm.state.registers[2] = U256::from(u32::MAX);

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
}

#[test]
#[ignore = "long-running; run on demand"]
fn callstack_saturation_should_mask_near_call_to_panic() {
    // In zk_evm, callstack-full is checked before opcode execution and masked into panic.
    // vm2 currently executes NearCall directly even when the callstack is saturated.
    // This test is long-running because it must fill the callstack up to VM_MAX_STACK_DEPTH.
    // Run on demand with:
    // cargo test -p zksync_vm2 callstack_saturation_should_mask_near_call_to_panic -- --ignored --nocapture
    let near_call = Instruction::from_near_call(
        Register1(Register::new(1)),
        Immediate1(0),
        crate::addressing_modes::Immediate2(0),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![near_call], vec![]);
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    vm.state.registers[1] = U256::zero();
    let snapshot = vm.world_diff.snapshot();
    for _ in 0..VM_MAX_STACK_DEPTH {
        vm.state
            .current_frame
            .push_near_call(vm.state.current_frame.gas, 0, snapshot.clone());
    }

    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_eq!(
        vm.state.current_frame.near_calls.len(),
        VM_MAX_STACK_DEPTH as usize - 1
    );
}

#[test]
fn non_kernel_returndata_forward_to_older_page_should_panic() {
    // zk_evm rejects non-kernel returndata forwarding to an older memory page.
    // vm2 only blocks forwarding to the current calldata page.
    let caller_program = Program::from_raw(
        vec![Instruction::from_ret(
            Register1(Register::new(1)),
            None,
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        )],
        vec![],
    );
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        caller_program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let caller_heap = vm.state.current_frame.heap;
    let caller_aux_heap = vm.state.current_frame.aux_heap;
    let callee_program = Program::from_raw(
        vec![Instruction::from_ret(
            Register1(Register::new(1)),
            None,
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        )],
        vec![],
    );
    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        callee_program,
        200_000,
        0,
        false,
        false,
        caller_heap,
        vm.world_diff.snapshot(),
    );

    let mut return_abi = FatPointer {
        offset: 0,
        memory_page: caller_aux_heap,
        start: 0,
        length: 0,
    }
    .into_u256();
    // ForwardFatPointer mode in ABI.
    return_abi.0[3] = 1_u64 << 32;
    vm.state.registers[1] = return_abi;
    vm.state.register_pointer_flags = 1 << 1;

    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_eq!(vm.state.registers[1], U256::zero());
}

#[test]
fn nonfresh_decommit_should_reuse_existing_memory_page() {
    // zk_evm reuses the same memory page for repeated decommit of the same code hash.
    // vm2 currently allocates a fresh page every time.
    let contract = (
        non_kernel_address(),
        Program::from_raw(vec![ret_instruction()], vec![]),
    );
    let mut world = TestWorld::new(&[contract]);
    let code_hash = *world
        .address_to_hash
        .values()
        .next()
        .expect("test contract hash must exist");

    let decommit_first = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let decommit_second = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(4)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![decommit_first, decommit_second], vec![]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );
    vm.state.registers[1] = code_hash;
    vm.state.registers[2] = U256::zero();

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let first = FatPointer::from(vm.state.registers[3]);

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let second = FatPointer::from(vm.state.registers[4]);

    assert_eq!(first.memory_page, second.memory_page);
}
