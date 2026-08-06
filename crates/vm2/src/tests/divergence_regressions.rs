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
    addressing_modes::{
        Arguments, CodePage, Immediate1, Register, Register1, Register2, RegisterAndImmediate,
    },
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

fn load_code_page_word<T: Tracer, W: World<T>>(
    index: u16,
    destination: Register,
) -> Instruction<T, W> {
    Instruction::from_add(
        CodePage(RegisterAndImmediate {
            immediate: index,
            register: Register::new(0),
        })
        .into(),
        Register2(Register::new(0)),
        Register1(destination).into(),
        Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
        false,
        false,
    )
}

fn normal_far_call<T: Tracer, W: World<T>>(
    abi_register: Register,
    address_register: Register,
) -> Instruction<T, W> {
    Instruction::from_far_call::<opcodes::Normal>(
        Register1(abi_register),
        Register2(address_register),
        Immediate1(0),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    )
}

fn system_far_call_abi(gas_to_pass: u32) -> U256 {
    let mut abi = U256::zero();
    let system_call_bit_within_settings = 1_u64 << 24;
    abi.0[3] = u64::from(gas_to_pass) | (system_call_bit_within_settings << 32);
    abi
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

#[test]
fn pointer_read_after_failed_far_call_should_return_zero() {
    let failed_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1),
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
    let read_returned_pointer = Instruction::from_pointer_read(
        Register1(Register::new(1)),
        Register1(Register::new(3)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(
        vec![failed_call, read_returned_pointer, ret_instruction()],
        vec![],
    );
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // The missing callee makes `far_call` enter a panicking frame. The reference
    // VM still returns an empty fat pointer in r1, so reading through it must
    // behave like a zero-length read rather than crashing the host.
    let mut far_call_abi = U256::zero();
    far_call_abi.0[3] = 10_000;
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = far_call_abi;
    vm.state.registers[2] = U256::zero();

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[3], U256::zero());
    assert_eq!(vm.state.register_pointer_flags & (1 << 1), 1 << 1);
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

#[test]
fn precompile_output_write_should_materialize_unallocated_dynamic_page() {
    let heap_write = Instruction::from_heap_write(
        Register1(Register::new(1)).into(),
        Register2(Register::new(2)),
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
    let program = Program::from_raw(vec![heap_write, precompile_call, ret_instruction()], vec![]);
    let mut world = PrecompileSentinelWorld::default();

    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let output_page = heap_page_from_base(next_page_group(first_dynamic_base_page()));
    assert!(!vm.state.heaps.contains(output_page));

    let input_value = U256::from(0x33_u64);
    let expected_output = input_value + U256::one();

    // ABI: read 32 bytes from the current heap via page-zero sentinel, then write
    // one output word to a valid dynamic page that has not been allocated yet.
    let mut precompile_abi = U256::zero();
    precompile_abi.0[0] = 32_u64 << 32;
    precompile_abi.0[1] = 1_u64 << 32;
    precompile_abi.0[2] = u64::from(output_page.as_u32()) << 32;

    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = U256::zero();
    vm.state.registers[2] = input_value;
    vm.state.registers[4] = precompile_abi;
    vm.state.registers[5] = U256::zero();

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[6], U256::one());
    assert!(vm.state.heaps.contains(output_page));
    assert_eq!(vm.state.heaps[output_page].read_u256(0), expected_output);
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

    vm.pop_frame(None, None)
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

    vm.pop_frame(Some(kept_heap), None)
        .expect("nested frame must be present for pop");

    assert_eq!(vm.state.heaps[decommit_heap].read_u256(0), code_word);
}

#[test]
fn pop_frame_does_not_compact_a_kept_heap_the_dying_frame_does_not_own() {
    // Compaction is gated on the returned page belonging to the dying frame. A page that is
    // merely on the dying frame's keep-alive list is not owned by it: the list is filled from
    // whatever page the frame's *child* returned, so it can name a live ancestor's heap.
    // Widening the ownership test to cover it would reintroduce the kernel ret-forward
    // divergence with a three-frame chain, so this must stay a no-op.
    // The owned counterpart is `pop_frame_compacts_dying_frames_own_heap_to_returndata_window`.
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

    let kept_heap = allocate_standalone_heap(&mut vm, &[]);
    let marker = U256::from(0xdead_beef_u64);
    // Three distant chunks; only the middle one is inside the returned window.
    vm.state.heaps.write_u256(kept_heap, 0, marker);
    vm.state.heaps.write_u256(kept_heap, 5000, marker);
    vm.state.heaps.write_u256(kept_heap, 9000, marker);
    vm.state
        .current_frame
        .heaps_i_am_keeping_alive
        .push(kept_heap);
    assert_ne!(kept_heap, vm.state.current_frame.heap);
    assert_ne!(kept_heap, vm.state.current_frame.aux_heap);

    // Return a 32-byte window at offset 5000.
    vm.pop_frame(Some(kept_heap), Some((5000, 32)))
        .expect("nested frame must be present for pop");

    // Every byte survives: the page is not the dying frame's to free.
    assert_eq!(vm.state.heaps[kept_heap].read_u256(5000), marker);
    assert_eq!(vm.state.heaps[kept_heap].read_u256(0), marker);
    assert_eq!(vm.state.heaps[kept_heap].read_u256(9000), marker);
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

#[test]
fn rollback_should_deallocate_dynamic_decommit_page_from_nested_frame() {
    let validation_address = Address::from_low_u64_be(2);
    let decommitter_address = Address::from_low_u64_be(3);
    let target_address = Address::from_low_u64_be(4);

    let validation_program = Program::from_raw(
        vec![
            load_code_page_word(0, Register::new(1)),
            load_code_page_word(1, Register::new(2)),
            normal_far_call(Register::new(1), Register::new(2)),
            ret_instruction(),
        ],
        vec![
            system_far_call_abi(1_000_000),
            U256::from(decommitter_address.to_low_u64_be()),
        ],
    );
    let decommitter_program = Program::from_raw(
        vec![
            Instruction::from_decommit(
                Register1(Register::new(3)),
                Register2(Register::new(2)),
                Register1(Register::new(4)),
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ),
            ret_instruction(),
        ],
        vec![],
    );
    let target_program = Program::from_raw(vec![ret_instruction()], vec![U256::from(0xfeed_u64)]);

    let mut world = TestWorld::new(&[
        (validation_address, validation_program),
        (decommitter_address, decommitter_program),
        (target_address, target_program),
    ]);
    let target_hash = *world
        .address_to_hash
        .get(&U256::from(target_address.to_low_u64_be()))
        .expect("target contract hash must exist");

    let bootloader_program = Program::from_raw(
        vec![
            normal_far_call(Register::new(1), Register::new(2)),
            ret_instruction(),
        ],
        vec![],
    );
    let mut vm = VirtualMachine::new(
        kernel_address(),
        bootloader_program,
        Address::zero(),
        &[],
        10_000_000,
        default_settings(),
    );
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = system_far_call_abi(5_000_000);
    vm.state.registers[2] = U256::from(validation_address.to_low_u64_be());
    vm.state.registers[3] = target_hash;

    vm.make_snapshot();
    let first_run_end = vm.run(&mut world, &mut ());
    assert!(
        matches!(first_run_end, ExecutionEnd::ProgramFinished(_)),
        "first run should finish cleanly: {first_run_end:?}"
    );

    // The decommitter frame is the second far-call frame. Its Decommit opcode materializes the
    // target code into that frame's own heap, then frame popping keeps the heap alive only through
    // the global decommit pin.
    let decommitter_base = next_page_group(first_dynamic_base_page());
    let decommitter_heap = heap_page_from_base(decommitter_base);
    assert_eq!(
        vm.world_diff.decommit_page(target_hash),
        Some(decommitter_heap)
    );
    assert!(vm.world_diff.is_decommit_page_pinned(decommitter_heap));
    assert!(vm.state.heaps.contains(decommitter_heap));
    assert!(!vm
        .state
        .current_frame
        .heaps_i_am_keeping_alive
        .contains(&decommitter_heap));

    vm.rollback();
    assert!(!vm.world_diff.is_decommit_page_pinned(decommitter_heap));
    assert!(
        !vm.state.heaps.contains(decommitter_heap),
        "rollback must sweep dynamic heaps that were reachable only through post-snapshot pins"
    );

    let replay_end = vm.run(&mut world, &mut ());
    assert!(
        matches!(replay_end, ExecutionEnd::ProgramFinished(_)),
        "replay after rollback should not hit a dynamic heap allocation conflict: {replay_end:?}"
    );
    assert_eq!(
        vm.world_diff.decommit_page(target_hash),
        Some(decommitter_heap)
    );
    assert!(vm.state.heaps.contains(decommitter_heap));
}

/// Builds a register-only `add` with an arbitrary `dst1` register index by patching a raw
/// encoding, the same way `zk_evm`'s `test_dst0_dst1_alias_on_add` patches bytecode. `add`
/// produces no second output, so `dst1` is a "dirty" field that must be cleared after execution.
fn dirty_add_instruction(
    src0_reg: u8,
    dst0_reg: u8,
    dst1_reg: u8,
) -> Instruction<(), TestWorld<()>> {
    let variant = OPCODES_TABLE
        .iter()
        .copied()
        .find(|variant| {
            matches!(variant.opcode, Opcode::Add(_))
                && matches!(
                    variant.src0_operand_type,
                    Operand::RegOnly
                        | Operand::RegOrImm(RegOrImmFlags::UseRegOnly)
                        | Operand::Full(ImmMemHandlerFlags::UseRegOnly)
                )
                && matches!(
                    variant.dst0_operand_type,
                    Operand::RegOnly | Operand::Full(ImmMemHandlerFlags::UseRegOnly)
                )
        })
        .expect("register-only Add variant must exist");

    let encoded = DecodedOpcode::<8, EncodingModeProduction> {
        variant,
        condition: Condition::Always,
        src0_reg_idx: src0_reg,
        src1_reg_idx: 0,
        dst0_reg_idx: dst0_reg,
        dst1_reg_idx: dst1_reg,
        imm_0: 0,
        imm_1: 0,
    }
    .serialize_as_integer();

    decode(encoded, false)
}

#[test]
fn dirty_dst1_on_add_is_cleared_to_zero() {
    // `add` has no second output, so a non-`r0` `dst1` register must be zeroed after execution,
    // matching zk_evm (see zksync-protocol#222). Here `dst0` and `dst1` both alias `r2`: the
    // `dst0` write happens first, then the `dst1` clear wins, leaving `r2 == 0`.
    let program = Program::from_raw(
        vec![dirty_add_instruction(1, 2, 2), ret_instruction()],
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

    // r1 is the addend; r2 starts non-zero so we can tell "add ran then cleared" (=> 0) apart from
    // "add wrote 7 but clear was skipped" (=> 7) and "instruction never touched r2" (=> 0xAA).
    vm.state.registers[1] = U256::from(7);
    vm.state.registers[2] = U256::from(0xAA);

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[2], U256::zero());
}

#[test]
fn dirty_dst1_on_add_without_alias_is_cleared_to_zero() {
    // Same as above but `dst0` (r3) and `dst1` (r2) are distinct: the add result lands in r3 and
    // the untouched `dst1` register r2 is cleared, regardless of its previous contents.
    let program = Program::from_raw(
        vec![dirty_add_instruction(1, 3, 2), ret_instruction()],
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

    vm.state.registers[1] = U256::from(7);
    vm.state.registers[2] = U256::from(0xAA);

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[3], U256::from(7));
    assert_eq!(vm.state.registers[2], U256::zero());
}

#[test]
fn uma_read_increment_panic_clears_dst1() {
    // A heap read with increment writes its incremented pointer to `dst1`, but panics (here on an
    // out-of-range offset) before it gets there. zk_evm still leaves `dst1` cleared to zero, so vm2
    // must too. This is the near-call-observable divergence from the original report.
    let heap_read = Instruction::from_heap_read(
        Register1(Register::new(1)).into(),
        Register1(Register::new(3)),
        Some(Register2(Register::new(4))),
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    let program = Program::from_raw(vec![heap_read], vec![]);
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // r1 holds an offset past the last addressable byte, forcing a panic before the increment.
    vm.state.registers[1] = U256::from(u32::MAX);
    // r4 (the increment / dst1 register) starts non-zero and with its pointer flag set.
    vm.state.registers[4] = U256::from(0xAA);
    vm.state.register_pointer_flags |= 1 << 4;

    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_eq!(vm.state.registers[4], U256::zero());
    assert_eq!(vm.state.register_pointer_flags & (1 << 4), 0);
}

#[test]
fn mul_second_output_is_preserved_not_cleared() {
    // The mirror of the "dirty dst1 is cleared" tests: `mul` genuinely produces a second output
    // (the high 256 bits go to `dst1`), so the post-execution clear must NOT fire and overwrite it.
    // `U256::MAX * 2 == 2^257 - 2`, giving low = `U256::MAX - 1` in `dst0` and high = 1 in `dst1`.
    let mul = Instruction::from_mul(
        Register1(Register::new(1)).into(), // src0 (low operand)
        Register2(Register::new(2)),        // src1
        Register1(Register::new(3)).into(), // dst0 (low word)
        Register2(Register::new(4)),        // dst1 (high word)
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        false,
        false,
    );
    let program = Program::from_raw(vec![mul, ret_instruction()], vec![]);
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
    vm.state.registers[1] = U256::MAX;
    vm.state.registers[2] = U256::from(2);
    // Sentinel: if the clear wrongly fired, `dst1` (r4) would be 0 instead of the real high word.
    vm.state.registers[4] = U256::from(0xAA);

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[3], U256::MAX - 1);
    assert_eq!(vm.state.registers[4], U256::one());
}

#[test]
fn unsatisfied_predicate_does_not_clear_dst1() {
    // A condition-skipped instruction must leave `dst1` untouched, matching zk_evm: there a failed
    // predicate masks the opcode into `Nop` with `dst1_reg_idx = 0`, so its unconditional post-apply
    // clear only ever zeroes `r0` (the discard register). vm2 reaches the same result by skipping the
    // clear entirely on the not-satisfied path. `mul` carries a real `dst1`; `IfGT` is unset here.
    let skipped_mul = Instruction::from_mul(
        Register1(Register::new(1)).into(),
        Register2(Register::new(2)),
        Register1(Register::new(3)).into(),
        Register2(Register::new(4)),
        Arguments::new(Predicate::IfGT, 5, ModeRequirements::none()),
        false,
        false,
    );
    let program = Program::from_raw(vec![skipped_mul, ret_instruction()], vec![]);
    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // Flags start cleared, so `IfGT` is not satisfied and the mul does not execute.
    vm.state.register_pointer_flags &= !(1 << 1);
    vm.state.registers[1] = U256::MAX;
    vm.state.registers[2] = U256::from(2);
    // `dst1` (r4) keeps its pre-existing value: the skip path must not zero it.
    vm.state.registers[4] = U256::from(0xAA);

    assert_eq!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::ProgramFinished(vec![])
    );
    assert_eq!(vm.state.registers[4], U256::from(0xAA));
    // dst0 (r3) is likewise untouched by the skipped instruction.
    assert_eq!(vm.state.registers[3], U256::zero());
}

/// A far `ret.panic` must still resolve its return-ABI pointer for *cost*: a fresh-heap pointer
/// whose `start + length` overflows `u32` grows the heap to `u32::MAX`, draining the frame's gas
/// to zero, matching the proving circuit / post-#217 `zk_evm` (see zksync-protocol#217). Before the
/// fix, vm2 short-circuited on panic and never parsed the return ABI, so the callee kept its gas
/// and returned it to the caller.
#[test]
fn far_ret_panic_charges_heap_growth_for_overflowing_pointer() {
    let callee_address = Address::from_low_u64_be(0x00C0_FFEE);
    // `ret.panic r3` — r3 carries the crafted return-ABI pointer (set once inside the callee frame).
    let panicking_program = Program::from_raw(
        vec![Instruction::from_panic(
            Register1(Register::new(3)),
            None,
            Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
        )],
        vec![],
    );
    let mut world = TestWorld::new(&[(callee_address, panicking_program)]);

    let gas_to_pass = 50_000;
    let mut far_call_abi = U256::zero();
    far_call_abi.0[3] = u64::from(gas_to_pass);
    let far_call = Instruction::from_far_call::<opcodes::Normal>(
        Register1(Register::new(1)),
        Register2(Register::new(2)),
        Immediate1(1), // exception handler -> caller instruction #1 (the trailing `ret`)
        false,
        false,
        Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
    );
    let caller_program = Program::from_raw(vec![far_call, ret_instruction()], vec![]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        caller_program,
        Address::zero(),
        &[],
        70_000,
        default_settings(),
    );

    vm.state.registers[1] = far_call_abi;
    vm.state.registers[2] = U256::from(callee_address.to_low_u64_be());
    vm.state.register_pointer_flags &= !(1 << 1);

    // Step 1: the far call pushes the callee frame (registers are cleared on entry).
    execute_one_instruction(&mut vm, &mut world, &mut ());
    assert_eq!(
        vm.state.current_frame.gas, gas_to_pass,
        "callee should receive the passed gas"
    );

    // Craft the return-ABI pointer in r3: start = u32::MAX, length = 1 (so start + length overflows
    // u32), MakeNewPointer / UseHeap (raw source byte 0), integer (no pointer flag).
    let mut ret_abi = U256::zero();
    ret_abi.0[1] = (1u64 << 32) | u64::from(u32::MAX); // length = 1 (bits 96..128), start = u32::MAX (bits 64..96)
    vm.state.registers[3] = ret_abi;
    vm.state.register_pointer_flags &= !(1 << 3);

    // Step 2: the callee's `ret.panic r3` returns to the caller's exception handler.
    execute_one_instruction(&mut vm, &mut world, &mut ());

    // Back in the caller frame: with the fix the callee drained all its gas on heap growth and
    // returned nothing, so the caller's remaining gas is well below the gas it passed in.
    assert!(
        vm.state.current_frame.gas < gas_to_pass,
        "caller gas {} should be below the {gas_to_pass} passed in — the callee's gas must have \
         been drained by u32::MAX heap growth",
        vm.state.current_frame.gas,
    );
}

// ---------------------------------------------------------------------------
// PR #116 returndata-window compaction: ownership of the compacted page.
// ---------------------------------------------------------------------------

/// `ret.ok r1` — forwards whatever return ABI sits in r1. `ret_instruction` above uses r0,
/// so it can never forward a pointer.
fn ret_r1_instruction<T: Tracer, W: World<T>>() -> Instruction<T, W> {
    Instruction::from_ret(
        Register1(Register::new(1)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    )
}

/// Loads a `ForwardFatPointer` return ABI over `[start, start + length)` of `page` into r1 and
/// sets r1's pointer flag — the register state a callee's [`ret_r1_instruction`] consumes when
/// it forwards a pointer rather than building a fresh one. Only r1's flag is touched: `Decommit`
/// leaves a live pointer in r3, and assigning the bitmap would silently clear it.
fn load_forward_ret_abi<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    page: HeapId,
    start: u32,
    length: u32,
) {
    let mut abi = FatPointer {
        offset: 0,
        memory_page: page,
        start,
        length,
    }
    .into_u256();
    abi.0[3] = 1_u64 << 32; // FatPointerSource::ForwardFatPointer
    vm.state.registers[1] = abi;
    vm.state.register_pointer_flags |= 1 << 1;
}

/// The same for a fresh `MakeNewPointer(ToHeap)` pointer, whose page `get_calldata` fills in from
/// the returning frame. It grows that frame's heap to `start + length` and charges for it.
fn load_new_heap_ret_abi<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    start: u32,
    length: u32,
) {
    let mut abi = FatPointer {
        offset: 0,
        memory_page: HeapId::from_u32_unchecked(0),
        start,
        length,
    }
    .into_u256();
    abi.0[3] = 0; // FatPointerSource::MakeNewPointer(ToHeap)
    vm.state.registers[1] = abi;
    vm.state.register_pointer_flags &= !(1 << 1);
}

/// Asserts r1 holds the fat pointer a `ret` was meant to return. Stronger than "r1 is non-zero",
/// which any successful return satisfies even if the pointer was re-targeted or zero-lengthed.
fn assert_returned_pointer<T: Tracer, W: World<T>>(
    vm: &VirtualMachine<T, W>,
    page: HeapId,
    start: u32,
    length: u32,
) {
    let returned = FatPointer::from(vm.state.registers[1]);
    assert_eq!(
        (returned.memory_page, returned.start, returned.length),
        (page, start, length)
    );
}

/// A kernel VM with one pushed frame — the dying one — both frames running `ret.ok r1`.
fn vm_with_pushed_kernel_frame() -> (VirtualMachine<(), TestWorld<()>>, TestWorld<()>) {
    let program: Program<(), TestWorld<()>> = Program::from_raw(vec![ret_r1_instruction()], vec![]);
    let mut vm = VirtualMachine::new(
        kernel_address(),
        program.clone(),
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );
    let calldata_heap = vm.state.current_frame.calldata_heap;
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        program,
        200_000,
        0,
        false,
        false,
        calldata_heap,
        vm.world_diff.snapshot(),
    );
    (vm, TestWorld::new(&[]))
}

/// A kernel callee that ret-forwards its `calldata_heap` pointer must not free any part
/// of the *victim* (still-live caller) frame's heap. `zk_evm` never frees a memory page, and
/// the victim reads its own heap through `HeapRead`, which no fat pointer bounds.
#[test]
fn kernel_ret_forward_must_not_compact_older_frame_heap() {
    let heap_read = Instruction::from_heap_read(
        Register1(Register::new(2)).into(),
        Register1(Register::new(5)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    // pc 0 is never executed by the victim: we set its pc to 1 so it resumes on the heap read.
    let victim_program: Program<(), TestWorld<()>> = Program::from_raw(
        vec![ret_instruction(), heap_read, ret_instruction()],
        vec![],
    );
    let callee_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_r1_instruction()], vec![]);

    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        victim_program.clone(),
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // The victim must be a *pushed* frame: the initial frame's heap is `BOOTLOADER_HEAP_PAGE`,
    // which `compact_to_window` skips as `is_always_allocated`, making the test vacuous.
    let initial_heap = vm.state.current_frame.heap;
    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        victim_program,
        500_000,
        0,
        false,
        false,
        initial_heap,
        vm.world_diff.snapshot(),
    );

    let victim_heap = vm.state.current_frame.heap;
    let marker_lo = U256::from(0x1111_1111_u64);
    let marker_in = U256::from(0x2222_2222_u64);
    let marker_hi = U256::from(0x3333_3333_u64);
    vm.state.heaps.write_u256(victim_heap, 0, marker_lo);
    vm.state.heaps.write_u256(victim_heap, 1024, marker_in);
    vm.state.heaps.write_u256(victim_heap, 5000, marker_hi);
    // So the victim's own `HeapRead` is in bounds after it resumes.
    vm.state.current_frame.heap_size = 8192;
    vm.state.current_frame.set_pc_from_u16(1);
    vm.state.registers[2] = U256::zero(); // HeapRead address 0

    // The kernel callee's calldata heap *is* the victim's live heap page — exactly what
    // `get_calldata`'s `MakeNewPointer(ToHeap)` branch produces for any heap far call.
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        callee_program,
        200_000,
        0,
        false,
        false,
        victim_heap,
        vm.world_diff.snapshot(),
    );
    assert!(vm.state.current_frame.is_kernel);
    assert_eq!(vm.state.current_frame.calldata_heap, victim_heap);

    load_forward_ret_abi(&mut vm, victim_heap, 1024, 32);

    // The callee's `ret.ok r1`.
    execute_one_instruction(&mut vm, &mut world, &mut ());

    // The kernel filter lets the forward through, so we really are back in the victim frame.
    assert_returned_pointer(&vm, victim_heap, 1024, 32);
    assert_eq!(vm.state.current_frame.heap, victim_heap);

    // The victim frame is still alive and reads its own heap through `HeapRead`, which is
    // bounded by `heap_size` only. Every byte must survive.
    assert_eq!(
        vm.state.heaps[victim_heap].read_u256(1024),
        marker_in,
        "in-window word must survive"
    );
    assert_eq!(
        vm.state.heaps[victim_heap].read_u256(0),
        marker_lo,
        "out-of-window word of a LIVE frame's heap must not be freed"
    );
    assert_eq!(
        vm.state.heaps[victim_heap].read_u256(5000),
        marker_hi,
        "out-of-window word of a LIVE frame's heap must not be freed"
    );

    // ... and through the victim's own `HeapRead` at address 0.
    execute_one_instruction(&mut vm, &mut world, &mut ());
    assert_eq!(
        vm.state.registers[5], marker_lo,
        "the victim's own HeapRead must not observe a freed chunk"
    );
}

/// The non-kernel counterpart. The `naked_ret` filter converts the same forward into a
/// panic, which is what makes the bug kernel-only. Locks that in.
#[test]
fn non_kernel_ret_forward_of_calldata_heap_should_panic() {
    let victim_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_instruction()], vec![]);
    let callee_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_r1_instruction()], vec![]);

    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        victim_program.clone(),
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let initial_heap = vm.state.current_frame.heap;
    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        victim_program,
        500_000,
        0,
        false,
        false,
        initial_heap,
        vm.world_diff.snapshot(),
    );

    let victim_heap = vm.state.current_frame.heap;
    let marker = U256::from(0x1111_1111_u64);
    vm.state.heaps.write_u256(victim_heap, 0, marker);
    vm.state.heaps.write_u256(victim_heap, 1024, marker);
    vm.state.heaps.write_u256(victim_heap, 5000, marker);

    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        callee_program,
        200_000,
        0,
        false,
        false,
        victim_heap,
        vm.world_diff.snapshot(),
    );
    assert!(!vm.state.current_frame.is_kernel);

    load_forward_ret_abi(&mut vm, victim_heap, 1024, 32);

    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_eq!(
        vm.state.registers[1],
        U256::zero(),
        "non-kernel forward of the calldata heap must be converted to a panic"
    );
    assert_eq!(vm.state.heaps[victim_heap].read_u256(0), marker);
    assert_eq!(vm.state.heaps[victim_heap].read_u256(1024), marker);
    assert_eq!(vm.state.heaps[victim_heap].read_u256(5000), marker);
}

/// The owned case must still compact, for both arms of the gate's `heap == dying.heap || aux_heap`.
#[test]
fn pop_frame_compacts_dying_frames_own_heaps_to_returndata_window() {
    for use_aux_heap in [false, true] {
        let (mut vm, _) = vm_with_pushed_kernel_frame();
        let page = if use_aux_heap {
            vm.state.current_frame.aux_heap
        } else {
            vm.state.current_frame.heap
        };
        let marker = U256::from(0xdead_beef_u64);
        for offset in [0, 5000, 9000] {
            vm.state.heaps.write_u256(page, offset, marker);
        }

        vm.pop_frame(Some(page), Some((5000, 32)))
            .expect("nested frame must be present for pop");

        assert_eq!(vm.state.heaps[page].read_u256(5000), marker, "in-window");
        for outside in [0, 9000] {
            let read = vm.state.heaps[page].read_u256(outside);
            assert_eq!(read, U256::zero(), "outside the window");
        }
    }
}

/// Covers the compaction *wiring*: the test above hands `pop_frame` a hand-built window, so a change
/// to the `(page, (start, length))` pair `naked_ret` derives from the returned pointer would leave it
/// green while the optimization silently stopped working.
#[test]
fn ret_with_new_pointer_compacts_dying_frames_own_heap() {
    let (mut vm, mut world) = vm_with_pushed_kernel_frame();
    let own_heap = vm.state.current_frame.heap;
    let marker = U256::from(0xdead_beef_u64);
    for offset in [0, 5000, 9000] {
        vm.state.heaps.write_u256(own_heap, offset, marker);
    }

    // The callee returns a fresh pointer over [5000, 5032) of its own heap.
    load_new_heap_ret_abi(&mut vm, 5000, 32);
    execute_one_instruction(&mut vm, &mut world, &mut ());

    assert_returned_pointer(&vm, own_heap, 5000, 32);
    assert_eq!(
        vm.state.heaps[own_heap].read_u256(5000),
        marker,
        "in-window"
    );
    for outside in [0, 9000] {
        let read = vm.state.heaps[own_heap].read_u256(outside);
        assert_eq!(read, U256::zero(), "outside the window");
    }
}

/// The state left by [`keepalive_chain_with_live_grandparent_heap`].
struct KeepaliveChain {
    vm: VirtualMachine<(), TestWorld<()>>,
    world: TestWorld<()>,
    /// `G`'s heap page — a live grandparent's dynamic page, now in `A`'s keep-alive list.
    hg: HeapId,
    /// Written to `hg` at offsets 0, 1024 and 5000.
    marker: U256,
}

/// Drives `frame0 -> G (dynamic heap Hg) -> A -> B (kernel)` and has `B` ret-forward the
/// pointer naming `Hg`, leaving execution back in `A` — whose `heaps_i_am_keeping_alive` now
/// holds `Hg`, a **live grandparent's** heap page.
///
/// `A` is deliberately non-kernel: the kernel restriction lives in `ret` (forwarding up), not
/// in `get_calldata` (forwarding down), so `A` can forward its calldata pointer downward.
/// `G` is left at pc 1, ready to execute its own `HeapRead` of address 0 into r5, with
/// `heap_size` set so that read is in bounds rather than bounding out.
fn keepalive_chain_with_live_grandparent_heap() -> KeepaliveChain {
    let heap_read = Instruction::from_heap_read(
        Register1(Register::new(2)).into(),
        Register1(Register::new(5)),
        None,
        Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
    );
    // `G` resumes at pc 1 on its own `HeapRead`.
    let g_program: Program<(), TestWorld<()>> = Program::from_raw(
        vec![ret_instruction(), heap_read, ret_instruction()],
        vec![],
    );
    let a_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_r1_instruction()], vec![]);
    let b_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_r1_instruction()], vec![]);

    let mut world = TestWorld::new(&[]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        g_program.clone(),
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    // frame0 (initial, BOOTLOADER_HEAP_PAGE) -> G (dynamic heap)
    let initial_heap = vm.state.current_frame.heap;
    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        g_program,
        700_000,
        0,
        false,
        false,
        initial_heap,
        vm.world_diff.snapshot(),
    );
    let hg = vm.state.current_frame.heap;
    let marker = U256::from(0x1111_1111_u64);
    vm.state.heaps.write_u256(hg, 0, marker);
    vm.state.heaps.write_u256(hg, 1024, marker);
    vm.state.heaps.write_u256(hg, 5000, marker);
    vm.state.current_frame.heap_size = 8192;
    vm.state.current_frame.set_pc_from_u16(1);
    vm.state.registers[2] = U256::zero();

    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        a_program,
        400_000,
        0,
        false,
        false,
        hg, // A.calldata_heap == Hg
        vm.world_diff.snapshot(),
    );
    let a_heap = vm.state.current_frame.heap;
    assert!(!vm.state.current_frame.is_kernel, "A must be non-kernel");

    // A -> B (kernel), with A forwarding its calldata pointer down as B's calldata.
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        b_program,
        200_000,
        0,
        false,
        false,
        hg, // B.calldata_heap == Hg (what ForwardFatPointer down produces)
        vm.world_diff.snapshot(),
    );
    assert!(vm.state.current_frame.is_kernel, "B must be kernel");

    // B rets a forwarded pointer naming Hg.
    load_forward_ret_abi(&mut vm, hg, 1024, 32);
    execute_one_instruction(&mut vm, &mut world, &mut ());
    assert_eq!(vm.state.current_frame.heap, a_heap, "must be back in A");

    KeepaliveChain {
        vm,
        world,
        hg,
        marker,
    }
}

/// Two properties of the same post-state, asserted after `B`'s pop and before `A` returns: the
/// ownership gate leaves a **live grandparent's** heap page intact even though `B` ret-forwarded a
/// narrow pointer naming it, and that page lands in `A`'s keep-alive list — which `pop_frame`
/// consumes as pages `A` may free, the precondition the deallocation defect below rests on.
#[test]
fn kernel_ret_forward_puts_live_grandparent_heap_in_callers_keepalive() {
    let KeepaliveChain { vm, hg, marker, .. } = keepalive_chain_with_live_grandparent_heap();

    assert_returned_pointer(&vm, hg, 1024, 32);
    assert!(
        vm.state
            .current_frame
            .heaps_i_am_keeping_alive
            .contains(&hg),
        "A's keep-alive must hold Hg, a live ancestor's page"
    );
    assert!(vm.state.heaps.contains(hg));
    for offset in [0, 1024, 5000] {
        assert_eq!(
            vm.state.heaps[hg].read_u256(offset),
            marker,
            "a live grandparent's heap must not be compacted"
        );
    }
}

/// When `A` then returns something other than `Hg`, `pop_frame`'s deallocation walk frees `Hg`
/// while `G` is still live, and `G` resumes reading zeros from its own heap.
///
/// Asserts the CURRENT, DEFECTIVE outcome deliberately, so a fix shows up here as a failure rather
/// than passing unnoticed — when that happens, rewrite this test, do not relax it. `zk_evm` never
/// frees a page, so the correct outcome is an intact `Hg`. Pre-existing on this PR's base and not
/// closed by the ownership gate: `extend(heap_to_keep)` ingests the child's returned page into the
/// *parent's* keep-alive list without checking the parent may free it.
#[test]
fn keepalive_deallocation_frees_live_grandparent_heap() {
    let KeepaliveChain {
        mut vm,
        mut world,
        hg,
        ..
    } = keepalive_chain_with_live_grandparent_heap();

    // A returns something that is NOT Hg: a fresh pointer into A's own heap, so the
    // `Some(heap) != heap_to_keep` test in `pop_frame`'s deallocation loop does not skip the
    // keep-alive entry.
    load_new_heap_ret_abi(&mut vm, 0, 32);
    execute_one_instruction(&mut vm, &mut world, &mut ());

    // Back in G, which is still live and about to read its own heap.
    assert_eq!(vm.state.current_frame.heap, hg, "must be back in G");
    assert!(
        !vm.state.heaps.contains(hg),
        "pre-existing defect: A's pop deallocates a live grandparent's heap page"
    );
    assert_eq!(vm.state.heaps[hg].read_u256(0), U256::zero());
    assert_eq!(vm.state.heaps[hg].read_u256(1024), U256::zero());
    assert_eq!(vm.state.heaps[hg].read_u256(5000), U256::zero());
    assert_eq!(
        vm.state.current_frame.heap_size, 8192,
        "no gas, bounds or panic signal accompanies the loss"
    );

    // G's own HeapRead at address 0 observes zero where it wrote a marker.
    execute_one_instruction(&mut vm, &mut world, &mut ());
    assert_eq!(vm.state.registers[5], U256::zero());
}

/// Verifies the precondition the keep-alive chain above rests on: the kernel restriction lives in `ret`
/// (forwarding returndata *up*), not in `get_calldata` (forwarding calldata *down*). A
/// NON-kernel frame can therefore hand its own calldata pointer — naming a live ancestor's
/// dynamic heap page — down to a kernel callee, via a real `FarCall` with
/// `FatPointerSource::ForwardFatPointer` and the production `ptr.pack` idiom.
#[test]
fn non_kernel_frame_can_forward_calldata_pointer_downward() {
    let a_address = Address::repeat_byte(2); // non-kernel
    let b_address = kernel_address(); // kernel

    let mut forward_abi_high = U256::zero();
    forward_abi_high.0[3] = 0x0003_0d40_u64 | (1_u64 << 32); // gas 200_000 | ForwardFatPointer

    // A: pack the ForwardFatPointer far-call ABI onto the calldata pointer it received in r1,
    // then far call the kernel contract B with it.
    let a_program = Program::from_raw(
        vec![
            load_code_page_word(0, Register::new(2)),
            Instruction::from_pointer_pack(
                Register1(Register::new(1)).into(),
                Register2(Register::new(2)),
                Register1(Register::new(1)).into(),
                Arguments::new(Predicate::Always, 7, ModeRequirements::none()),
                false,
            ),
            load_code_page_word(1, Register::new(3)),
            normal_far_call(Register::new(1), Register::new(3)),
            ret_instruction(),
        ],
        vec![forward_abi_high, address_into_u256(b_address)],
    );
    let b_program = Program::from_raw(vec![ret_instruction()], vec![]);
    let g_program: Program<(), TestWorld<()>> = Program::from_raw(
        vec![
            normal_far_call(Register::new(1), Register::new(2)),
            ret_instruction(),
        ],
        vec![],
    );

    let mut world = TestWorld::new(&[(a_address, a_program), (b_address, b_program)]);
    let mut vm = VirtualMachine::new(
        non_kernel_address(),
        g_program.clone(),
        Address::zero(),
        &[],
        3_000_000,
        default_settings(),
    );

    // G must be a pushed frame so its heap is a dynamic page, as in the keep-alive chain.
    let initial_heap = vm.state.current_frame.heap;
    vm.push_frame::<opcodes::Normal>(
        non_kernel_address(),
        g_program,
        2_000_000,
        0,
        false,
        false,
        initial_heap,
        vm.world_diff.snapshot(),
    );
    let hg = vm.state.current_frame.heap;
    vm.state
        .heaps
        .write_u256(hg, 0, U256::from(0x1111_1111_u64));

    // G far calls A with a fresh heap calldata pointer: MakeNewPointer / ToHeap, [0, 128).
    let mut g_abi = FatPointer {
        offset: 0,
        memory_page: HeapId::from_u32_unchecked(0),
        start: 0,
        length: 128,
    }
    .into_u256();
    g_abi.0[3] = 1_000_000_u64; // gas, source byte 0 = MakeNewPointer / ToHeap
    vm.state.registers[1] = g_abi;
    vm.state.registers[2] = address_into_u256(a_address);
    vm.state.register_pointer_flags &= !(1 << 1);

    execute_one_instruction(&mut vm, &mut world, &mut ()); // G's FarCall

    assert!(!vm.state.current_frame.is_kernel, "A must be non-kernel");
    assert_eq!(
        vm.state.current_frame.calldata_heap, hg,
        "A's calldata heap must be G's live dynamic heap page"
    );
    let a_calldata_pointer = FatPointer::from(vm.state.registers[1]);
    assert_eq!(a_calldata_pointer.memory_page, hg);

    // A: load ABI high limbs, ptr.pack, load B's address, far call B.
    for _ in 0..4 {
        execute_one_instruction(&mut vm, &mut world, &mut ());
    }

    assert!(vm.state.current_frame.is_kernel, "B must be kernel");
    assert_eq!(
        vm.state.current_frame.calldata_heap, hg,
        "a NON-kernel frame must be able to forward its calldata pointer down to a kernel \
         callee, so the kernel callee's calldata heap is a live ancestor's page"
    );
}

/// The decommit pin still protects an *owned* page. `Decommit` passes
/// `current_frame.heap` as the candidate page, so a decommit page can be the dying frame's own
/// heap — the ownership test passes and the pin is the only remaining guard. This is the shape
/// `CodeOracle.yul` uses (`active_ptr_shrink_assign` then `active_ptr_return_forward`), and
/// other frames read the shared code page in full.
#[test]
fn decommit_pin_protects_owned_page_from_returndata_compaction() {
    let code_word = U256::from(0xaaaa_aaaa_u64);
    // 60 code words = 1920 bytes, spanning several 256-byte chunks.
    let contract = (
        non_kernel_address(),
        Program::from_raw(vec![ret_instruction()], vec![code_word; 60]),
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
    let outer_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![ret_instruction()], vec![]);
    let decommitting_program: Program<(), TestWorld<()>> =
        Program::from_raw(vec![decommit, ret_r1_instruction()], vec![]);

    let mut vm = VirtualMachine::new(
        kernel_address(),
        outer_program,
        Address::zero(),
        &[],
        1_000_000,
        default_settings(),
    );

    let caller_heap = vm.state.current_frame.heap;
    vm.push_frame::<opcodes::Normal>(
        kernel_address(),
        decommitting_program,
        500_000,
        0,
        false,
        false,
        caller_heap,
        vm.world_diff.snapshot(),
    );

    vm.state.registers[1] = code_hash;
    vm.state.registers[2] = U256::zero();
    execute_one_instruction(&mut vm, &mut world, &mut ()); // the callee's `Decommit`

    let decommit_page = FatPointer::from(vm.state.registers[3]).memory_page;
    assert_eq!(
        decommit_page, vm.state.current_frame.heap,
        "the decommit candidate page is the dying frame's own heap, so it is owned"
    );
    assert!(vm.world_diff.is_decommit_page_pinned(decommit_page));
    assert_eq!(vm.state.heaps[decommit_page].read_u256(0), code_word);
    assert_eq!(vm.state.heaps[decommit_page].read_u256(1024), code_word);

    // Ret-forward a pointer narrowed to 32 bytes at offset 1024, as `CodeOracle` does.
    load_forward_ret_abi(&mut vm, decommit_page, 1024, 32);
    assert_ne!(
        vm.state.register_pointer_flags & (1 << 3),
        0,
        "the return-ABI setup must leave the r3 pointer `Decommit` produced tagged"
    );

    execute_one_instruction(&mut vm, &mut world, &mut ()); // the callee's `ret.ok r1`

    assert_eq!(
        vm.state.current_frame.heap, caller_heap,
        "back in the caller"
    );
    assert!(
        vm.world_diff.is_decommit_page_pinned(decommit_page),
        "no return path can unpin a decommit page"
    );
    assert_eq!(
        vm.state.heaps[decommit_page].read_u256(1024),
        code_word,
        "in-window code bytes survive"
    );
    assert_eq!(
        vm.state.heaps[decommit_page].read_u256(0),
        code_word,
        "a pinned decommit page must not be compacted even though the dying frame owns it"
    );
}
