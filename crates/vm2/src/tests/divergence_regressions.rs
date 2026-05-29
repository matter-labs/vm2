use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    decoding::EncodingModeProduction,
    ethereum_types::Address,
    system_params::{NEW_EVM_FRAME_MEMORY_STIPEND, NEW_FRAME_MEMORY_STIPEND, VM_MAX_STACK_DEPTH},
    Condition, DecodedOpcode, ImmMemHandlerFlags, Opcode, Operand, RegOrImmFlags, UMAOpcode,
    OPCODES_TABLE, UMA_INCREMENT_FLAG_IDX,
};
use zksync_vm2_interface::{opcodes, HeapId, Tracer};

use crate::{
    addressing_modes::{Arguments, Immediate1, Register, Register1, Register2},
    decode::decode,
    fat_pointer::FatPointer,
    instruction_handlers::address_into_u256,
    page_ids::{
        aux_heap_page_from_base, first_dynamic_base_page, heap_page_from_base, next_page_group,
    },
    precompiles::{PrecompileMemoryReader, PrecompileOutput, Precompiles},
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

fn allocate_standalone_heap<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    memory: &[u8],
) -> HeapId {
    let mut page = vm.state.next_base_page();
    loop {
        let heap = HeapId::from_u32_unchecked(page);
        if !vm.state.heaps.contains(heap) {
            vm.state.heaps.allocate_with_content_at(heap, memory);
            return heap;
        }
        page = next_page_group(page);
    }
}

fn ret_instruction<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(0)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

fn bytes32(value: U256) -> [u8; 32] {
    let mut bytes = [0; 32];
    value.to_big_endian(&mut bytes);
    bytes
}

fn masked_default_aa_far_call(version_byte: u8) -> VirtualMachine<(), TestWorld<()>> {
    let default_aa_address = Address::from_low_u64_be(0x100);
    let destination_address = non_kernel_address();
    let default_aa_program = Program::from_raw(vec![ret_instruction()], vec![]);
    let mut world = TestWorld::new(&[(default_aa_address, default_aa_program)]);

    let default_aa_hash = *world
        .address_to_hash
        .get(&address_into_u256(default_aa_address))
        .expect("default AA hash must be registered in test world");

    // The target storage slot describes code that cannot be called in this mode:
    // byte 1 marks the contract as still in construction, while the call below is
    // a regular non-constructor call. Reference zk_evm masks this to default AA but
    // still uses byte 0 when selecting the new frame memory stipend.
    let mut masked_code_info = [0; 32];
    masked_code_info[0] = version_byte;
    masked_code_info[1] = 1;
    world.address_to_hash.insert(
        address_into_u256(destination_address),
        U256::from_big_endian(&masked_code_info),
    );

    let far_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![far_call, ret_instruction()], vec![]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        Settings {
            default_aa_code_hash: bytes32(default_aa_hash),
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );

    let mut far_call_abi = U256::zero();
    far_call_abi.0[3] = 10_000;
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = far_call_abi;
    vm.state.registers[2] = address_into_u256(destination_address);
    vm.state.current_frame.is_static = true;

    execute_one_instruction(&mut vm, &mut world, &mut ());
    vm
}

#[test]
fn bootloader_calldata_pointer_should_use_reference_page_id() {
    let program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_instruction::<(), TestWorld<()>>()], vec![]);
    let vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[1, 2, 3, 4],
        1_000_000,
        default_settings(),
    );

    let calldata = FatPointer::from(vm.state.registers[1]);
    assert_eq!(
        calldata.memory_page,
        zksync_vm2_interface::HeapId::FIRST_CALLDATA
    );
    assert_eq!(calldata.length, 4);
}

#[test]
fn far_call_calldata_pointer_should_use_caller_heap_reference_page() {
    let called_address = Address::from_low_u64_be(2);
    let called_program = Program::from_raw(vec![ret_instruction()], vec![]);
    let mut world = TestWorld::new(&[(called_address, called_program)]);
    let called_address_as_u256 = U256::from(called_address.to_low_u64_be());

    let far_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![far_call, ret_instruction()], vec![]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let mut far_call_abi = U256::zero();
    far_call_abi.0[3] = 10_000;
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = far_call_abi;
    vm.state.registers[2] = called_address_as_u256;

    execute_one_instruction(&mut vm, &mut world, &mut ());

    let calldata = FatPointer::from(vm.state.registers[1]);
    assert_eq!(calldata.memory_page, zksync_vm2_interface::HeapId::FIRST);
    assert_eq!(
        vm.state.current_frame.heap,
        heap_page_from_base(first_dynamic_base_page())
    );
    assert_eq!(
        vm.state.current_frame.aux_heap,
        aux_heap_page_from_base(first_dynamic_base_page())
    );
}

#[test]
fn masked_evm_blob_far_call_should_keep_evm_stipend() {
    let vm = masked_default_aa_far_call(2);

    assert_eq!(
        vm.state.current_frame.heap_size,
        NEW_EVM_FRAME_MEMORY_STIPEND
    );
    assert_eq!(
        vm.state.current_frame.aux_heap_size,
        NEW_EVM_FRAME_MEMORY_STIPEND
    );
    assert!(
        vm.state.current_frame.is_static,
        "masked EVM blob calls should keep default-AA static behavior; only the stipend uses the blob version byte"
    );
}

#[test]
fn masked_native_far_call_should_keep_regular_stipend() {
    let vm = masked_default_aa_far_call(1);

    assert_eq!(vm.state.current_frame.heap_size, NEW_FRAME_MEMORY_STIPEND);
    assert_eq!(
        vm.state.current_frame.aux_heap_size,
        NEW_FRAME_MEMORY_STIPEND
    );
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
    // In zk_evm this opcode is executable in kernel mode. This regression test locks that
    // behavior in vm2.
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
    // In zk_evm this opcode is executable in kernel mode. This regression test locks that
    // behavior in vm2.
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

#[test]
fn static_memory_should_be_isolated_from_regular_heap() {
    let static_write = Instruction::from_static_memory_write(
        Register1(Register::new(1)).into(),
        Register2(Register::new(2)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let heap_read = Instruction::from_heap_read(
        Register1(Register::new(1)).into(),
        Register1(Register::new(3)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let static_read = Instruction::from_static_memory_read(
        Register1(Register::new(1)).into(),
        Register1(Register::new(4)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(
        vec![static_write, heap_read, static_read, ret_instruction()],
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

    let static_value = U256::from(0x11_u64);
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = U256::zero();
    vm.state.registers[2] = static_value;

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[3], U256::zero());
    assert_eq!(vm.state.registers[4], static_value);
}

fn assert_uma_read_increment_preserves_pointer_flag(
    read_instruction: Instruction<(), TestWorld<()>>,
    address: Address,
) {
    let program = Program::from_raw(vec![read_instruction], vec![]);
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        address,
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // Panic returndata is represented as an empty fat pointer: value zero, with
    // the pointer tag still set. UMA range checks accept that value, so the
    // incremented register must preserve the tag exactly as zk_evm does.
    vm.state.registers[1] = U256::zero();
    vm.state.register_pointer_flags = 1 << 1;

    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_eq!(vm.state.registers[3], U256::from(32));
    assert_eq!(vm.state.register_pointer_flags & (1 << 3), 1 << 3);
}

#[test]
fn uma_read_increment_should_preserve_source_pointer_flag() {
    assert_uma_read_increment_preserves_pointer_flag(
        Instruction::from_heap_read(
            Register1(Register::new(1)).into(),
            Register1(Register::new(2)),
            Some(Register2(Register::new(3))),
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        ),
        non_kernel_address(),
    );

    assert_uma_read_increment_preserves_pointer_flag(
        Instruction::from_aux_heap_read(
            Register1(Register::new(1)).into(),
            Register1(Register::new(2)),
            Some(Register2(Register::new(3))),
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        ),
        non_kernel_address(),
    );

    assert_uma_read_increment_preserves_pointer_flag(
        Instruction::from_static_memory_read(
            Register1(Register::new(1)).into(),
            Register1(Register::new(2)),
            Some(Register2(Register::new(3))),
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        ),
        kernel_address(),
    );
}

#[derive(Debug, Default)]
struct IncrementingPrecompiles;

impl Precompiles for IncrementingPrecompiles {
    fn call_precompile(
        &self,
        _: u16,
        mut memory: PrecompileMemoryReader<'_>,
        _: u64,
    ) -> PrecompileOutput {
        let mut input_word = [0_u8; 32];
        for byte in &mut input_word {
            *byte = memory.next().unwrap_or_default();
        }
        (U256::from_big_endian(&input_word) + U256::one()).into()
    }
}

#[derive(Debug, Default)]
struct PrecompileSentinelWorld {
    precompiles: IncrementingPrecompiles,
}

impl StorageInterface for PrecompileSentinelWorld {
    fn read_storage(&mut self, _: H160, _: U256) -> StorageSlot {
        StorageSlot::EMPTY
    }

    fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
        0
    }

    fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
        false
    }
}

impl<T: Tracer> World<T> for PrecompileSentinelWorld {
    fn decommit(&mut self, _: U256) -> Program<T, Self> {
        Program::new_panicking()
    }

    fn decommit_code(&mut self, _: U256) -> Vec<u8> {
        vec![]
    }

    fn precompiles(&self) -> &impl Precompiles {
        &self.precompiles
    }
}

#[test]
fn precompile_zero_memory_page_should_use_current_heap_instead_of_static_memory() {
    let static_write = Instruction::from_static_memory_write(
        Register1(Register::new(1)).into(),
        Register2(Register::new(2)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let heap_write = Instruction::from_heap_write(
        Register1(Register::new(1)).into(),
        Register2(Register::new(3)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        false,
    );
    let precompile_call = Instruction::from_precompile_call(
        Register1(Register::new(4)),
        Register2(Register::new(5)),
        Register1(Register::new(6)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let heap_read_after = Instruction::from_heap_read(
        Register1(Register::new(1)).into(),
        Register1(Register::new(7)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let static_read_after = Instruction::from_static_memory_read(
        Register1(Register::new(1)).into(),
        Register1(Register::new(8)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(
        vec![
            static_write,
            heap_write,
            precompile_call,
            heap_read_after,
            static_read_after,
            ret_instruction(),
        ],
        vec![],
    );
    let mut world = PrecompileSentinelWorld::default();

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let static_value = U256::from(0x11_u64);
    let heap_value = U256::from(0x22_u64);
    let expected_heap_after_precompile = heap_value + U256::one();

    // ABI: read 32 bytes from offset 0, write 1 word at offset 0, with page ids left at zero.
    // Page zero is the sentinel path under test.
    let mut precompile_abi = U256::zero();
    precompile_abi.0[0] = 32_u64 << 32;
    precompile_abi.0[1] = 1_u64 << 32;

    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = U256::zero();
    vm.state.registers[2] = static_value;
    vm.state.registers[3] = heap_value;
    vm.state.registers[4] = precompile_abi;
    vm.state.registers[5] = U256::zero();

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[6], U256::one());
    assert_eq!(vm.state.registers[7], expected_heap_after_precompile);
    assert_eq!(vm.state.registers[8], static_value);
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
#[ignore = "extreme callstack saturation case; memory-heavy and long-running; run on demand"]
fn callstack_saturation_should_mask_near_call_to_panic() {
    // This test checks the extreme case of callstack saturation, which is highly unlikely
    // to be hit in practice. It is memory-heavy and long-running.
    // Consider running it only when debugging an active VM issue and you suspect
    // callstack processing behavior.
    // In zk_evm, callstack-full is checked before opcode execution and masked into panic.
    // vm2 should preserve this behavior.
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
#[allow(clippy::similar_names)] // `caller` / `callee` is standard notation
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
fn fresh_decommit_should_use_current_heap_page() {
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

    let decommit = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![decommit], vec![]);

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
    let pointer = FatPointer::from(vm.state.registers[3]);

    assert_eq!(pointer.memory_page, vm.state.current_frame.heap);
    assert_eq!(
        vm.world_diff.decommit_page(code_hash),
        Some(pointer.memory_page)
    );
}

#[test]
fn nonfresh_decommit_should_reuse_existing_memory_page() {
    // zk_evm reuses the same memory page for repeated decommit of the same code hash.
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
    assert_eq!(first.memory_page, vm.state.current_frame.heap);

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let second = FatPointer::from(vm.state.registers[4]);

    assert_eq!(first.memory_page, second.memory_page);
}

#[test]
fn fresh_decommit_should_preserve_existing_heap_bytes_after_code() {
    let code_word = U256::from(0x363d_3d37_363d_34f0_u64);
    let contract = (
        non_kernel_address(),
        Program::from_raw(vec![ret_instruction()], vec![code_word]),
    );
    let mut world = TestWorld::new(&[contract]);
    let code_hash = *world
        .address_to_hash
        .values()
        .next()
        .expect("test contract hash must exist");

    let decommit = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![decommit], vec![]);

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

    let preserved = [0xda, 0x0a, 0x64, 0x56];
    let preserved_word = U256::from_big_endian(&[
        preserved[0],
        preserved[1],
        preserved[2],
        preserved[3],
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    let current_heap = vm.state.current_frame.heap;
    vm.state.heaps.write_u256(current_heap, 32, preserved_word);

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let pointer = FatPointer::from(vm.state.registers[3]);

    assert_eq!(pointer.memory_page, current_heap);
    assert_eq!(vm.state.heaps[current_heap].read_u256(0), code_word);
    assert_eq!(
        vm.state.heaps[current_heap].read_range_big_endian(32..36),
        preserved
    );
}

#[test]
fn decommit_after_far_call_decommit_should_not_panic() {
    // Far-call decommit must eagerly materialize a reusable decommit page.
    // Follow-up `Decommit` calls should return that same page without duplicate keep-alive entries.
    let called_address = Address::from_low_u64_be(2);
    let called_program = Program::from_raw(vec![ret_instruction()], vec![]);
    let mut world = TestWorld::new(&[(called_address, called_program)]);
    let called_address_as_u256 = U256::from(called_address.to_low_u64_be());
    let code_hash = *world
        .address_to_hash
        .get(&called_address_as_u256)
        .expect("test contract hash must exist");

    let far_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
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
    let program = Program::from_raw(
        vec![far_call, decommit_first, decommit_second, ret_instruction()],
        vec![],
    );

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let mut far_call_abi = U256::zero();
    far_call_abi.0[3] = 10_000;
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = far_call_abi;
    vm.state.registers[2] = called_address_as_u256;

    execute_one_instruction(&mut vm, &mut world, &mut ());
    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert!(
        vm.world_diff.decommit_page(code_hash).is_some(),
        "Far-call decommit should materialize a reusable page"
    );

    vm.state.registers[1] = code_hash;
    vm.state.registers[2] = U256::zero();
    vm.state.register_pointer_flags &= !(1 << 1);

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let first = FatPointer::from(vm.state.registers[3]);

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let second = FatPointer::from(vm.state.registers[4]);

    let keep_alive_occurrences = vm
        .state
        .current_frame
        .heaps_i_am_keeping_alive
        .iter()
        .filter(|&&heap| heap == first.memory_page)
        .count();

    assert!(
        vm.world_diff.decommit_page(code_hash).is_some(),
        "Non-fresh decommit should keep using a materialized reusable page"
    );
    assert_eq!(first.memory_page, second.memory_page);
    assert_eq!(
        keep_alive_occurrences, 1,
        "Reused decommit pages should be recorded in keep-alive once"
    );
}

#[test]
fn nonfresh_decommit_should_keep_page_alive_after_nested_frame_returns() {
    // Reusing decommit pages is correct only if the page survives nested frame teardown.
    let code_word = U256::from(0xdead_beef_u64);
    let contract = (
        non_kernel_address(),
        Program::from_raw(vec![ret_instruction()], vec![code_word]),
    );
    let mut world = TestWorld::new(&[contract]);
    let code_hash = *world
        .address_to_hash
        .values()
        .next()
        .expect("test contract hash must exist");

    let nested_decommit = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let nested_program = Program::from_raw(vec![nested_decommit], vec![]);
    let bootloader_decommit = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(4)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let bootloader_program = Program::from_raw(vec![bootloader_decommit], vec![]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        bootloader_program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );
    vm.state.registers[1] = code_hash;
    vm.state.registers[2] = U256::zero();

    let calldata_heap = vm.state.current_frame.calldata_heap;
    let world_before_nested = vm.world_diff.snapshot();
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        nested_program,
        200_000,
        0,
        false,
        false,
        calldata_heap,
        world_before_nested,
    );

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let first = FatPointer::from(vm.state.registers[3]);
    let nested_heap = vm.state.current_frame.heap;
    assert_eq!(vm.state.heaps[first.memory_page].read_u256(0), code_word);
    assert_eq!(first.memory_page, nested_heap);

    vm.pop_frame(None)
        .expect("nested frame must be present for pop");

    execute_one_instruction(&mut vm, &mut world, &mut ());
    let second = FatPointer::from(vm.state.registers[4]);
    let bootloader_heap = vm.state.current_frame.heap;

    let keep_alive_occurrences = vm
        .state
        .current_frame
        .heaps_i_am_keeping_alive
        .iter()
        .filter(|&&heap| heap == second.memory_page)
        .count();

    assert_eq!(first.memory_page, second.memory_page);
    assert_ne!(second.memory_page, bootloader_heap);
    assert_eq!(vm.state.heaps[second.memory_page].read_u256(0), code_word);
    assert!(vm.world_diff.is_decommit_page_pinned(second.memory_page));
    assert_eq!(
        keep_alive_occurrences, 0,
        "Pinned decommit pages owned by the current frame should not need a keep-alive entry"
    );
}

#[test]
fn decommit_page_in_keep_alive_list_should_not_be_deallocated_on_pop() {
    let program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_instruction::<(), TestWorld<()>>()], vec![]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program.clone(),
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let calldata_heap = vm.state.current_frame.calldata_heap;
    let world_before_nested = vm.world_diff.snapshot();
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        program,
        200_000,
        0,
        false,
        false,
        calldata_heap,
        world_before_nested,
    );

    let code_word = U256::from(0xabcdu64);
    let mut code_bytes = [0_u8; 32];
    code_word.to_big_endian(&mut code_bytes);
    let decommit_heap = allocate_standalone_heap(&mut vm, &code_bytes);
    let kept_heap = allocate_standalone_heap(&mut vm, &[0x11; 32]);

    vm.world_diff
        .set_decommit_page(U256::from(0x1234_u64), decommit_heap);
    vm.state
        .current_frame
        .heaps_i_am_keeping_alive
        .extend([decommit_heap, kept_heap]);

    vm.pop_frame(Some(kept_heap))
        .expect("nested frame must be present for pop");

    assert_eq!(vm.state.heaps[decommit_heap].read_u256(0), code_word);
}

#[test]
fn rollback_should_preserve_pre_snapshot_decommit_page() {
    let program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_instruction::<(), TestWorld<()>>()], vec![]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let code_word = U256::from(0xdead_beef_u64);
    let mut code_bytes = [0_u8; 32];
    code_word.to_big_endian(&mut code_bytes);
    let decommit_heap = allocate_standalone_heap(&mut vm, &code_bytes);
    vm.world_diff
        .set_decommit_page(U256::from(0xfeed_u64), decommit_heap);

    vm.make_snapshot();
    vm.state
        .current_frame
        .heaps_i_am_keeping_alive
        .push(decommit_heap);
    vm.rollback();

    assert_eq!(vm.state.heaps[decommit_heap].read_u256(0), code_word);
}

#[test]
fn rollback_should_restore_bootloader_heap_after_fresh_decommit() {
    let code_word = U256::from(0xdead_beef_u64);
    let contract = (
        non_kernel_address(),
        Program::from_raw(vec![ret_instruction()], vec![code_word]),
    );
    let mut world = TestWorld::new(&[contract]);
    let code_hash = *world
        .address_to_hash
        .values()
        .next()
        .expect("test contract hash must exist");

    let decommit = Instruction::from_decommit(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Register1(Register::new(3)),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let bootloader_program = Program::from_raw(vec![decommit], vec![]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        bootloader_program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let bootloader_heap = vm.state.current_frame.heap;
    let sentinel = U256::from(0x1234_5678_u64);
    vm.state.heaps.write_u256(bootloader_heap, 0, sentinel);
    vm.state.registers[1] = code_hash;
    vm.state.registers[2] = U256::zero();

    vm.make_snapshot();
    execute_one_instruction(&mut vm, &mut world, &mut ());
    assert_eq!(vm.state.heaps[bootloader_heap].read_u256(0), code_word);

    vm.rollback();
    assert_eq!(vm.state.heaps[bootloader_heap].read_u256(0), sentinel);
}
