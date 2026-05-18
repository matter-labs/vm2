use primitive_types::{H160, U256};
use zksync_vm2_interface::{HeapId, Tracer};

use super::{heap, stack::Stack, world::MockWorld};
use crate::{
    callframe::Callframe, page_ids::first_dynamic_base_page, predication::Flags, state::State,
    world_diff::WorldDiff, Settings, VirtualMachine,
};

#[derive(Debug, Clone)]
pub struct ScenarioVmConfig {
    pub raw_instructions: Vec<u64>,
    pub code_page: U256,
    pub registers: [(U256, bool); 16],
    pub flags: (bool, bool, bool),
    pub frame: ScenarioFrameConfig,
    pub stack: ScenarioStackConfig,
    pub memory: ScenarioMemoryConfig,
    pub storage_read: Option<U256>,
    pub transaction_number: u16,
    pub context_u128: u128,
    pub settings: Settings,
    pub include_parent_frame: bool,
}

impl Default for ScenarioVmConfig {
    fn default() -> Self {
        let frame = ScenarioFrameConfig::default();
        let mut default_aa_code_hash = [0; 32];
        default_aa_code_hash[0] = 1;
        let mut evm_interpreter_code_hash = [0; 32];
        evm_interpreter_code_hash[0] = 1;

        Self {
            raw_instructions: vec![0],
            code_page: U256::zero(),
            registers: [(U256::zero(), false); 16],
            flags: (false, false, false),
            stack: ScenarioStackConfig::default(),
            memory: ScenarioMemoryConfig {
                heap_id: frame.heap,
                heap_read_u256: U256::zero(),
            },
            frame,
            storage_read: Some(U256::zero()),
            transaction_number: 0,
            context_u128: 0,
            settings: Settings {
                default_aa_code_hash,
                evm_interpreter_code_hash,
                hook_address: 0,
            },
            include_parent_frame: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScenarioFrameConfig {
    pub address: H160,
    pub code_address: H160,
    pub caller: H160,
    pub exception_handler: u16,
    pub context_u128: u128,
    pub is_static: bool,
    pub sp: u16,
    pub gas: u32,
    pub base_page: u32,
    pub heap: HeapId,
    pub aux_heap: HeapId,
    pub calldata_heap: HeapId,
    pub heap_bound: u32,
    pub aux_heap_bound: u32,
}

impl Default for ScenarioFrameConfig {
    fn default() -> Self {
        let base_page = first_dynamic_base_page();
        Self {
            // Low addresses execute in kernel mode; this is usually the right default for
            // isolated opcode findings.
            address: H160::from_low_u64_be(1),
            code_address: H160::from_low_u64_be(1),
            caller: H160::zero(),
            exception_handler: 0,
            context_u128: 0,
            is_static: false,
            sp: 0,
            gas: 1_000_000,
            base_page,
            heap: HeapId::from_u32_unchecked(base_page + 2),
            aux_heap: HeapId::from_u32_unchecked(base_page + 3),
            calldata_heap: HeapId::FIRST_CALLDATA,
            heap_bound: 0,
            aux_heap_bound: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScenarioStackConfig {
    pub read_value: U256,
    pub read_is_pointer: bool,
}

impl Default for ScenarioStackConfig {
    fn default() -> Self {
        Self {
            read_value: U256::zero(),
            read_is_pointer: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScenarioMemoryConfig {
    pub heap_id: HeapId,
    pub heap_read_u256: U256,
}

pub fn build_scenario_vm(config: ScenarioVmConfig) -> (VirtualMachine<(), MockWorld>, MockWorld) {
    build_scenario_vm_with_tracer(config)
}

fn build_scenario_vm_with_tracer<T: Tracer>(
    config: ScenarioVmConfig,
) -> (VirtualMachine<T, MockWorld>, MockWorld) {
    let program = if config.raw_instructions.len() == 1 {
        crate::Program::from_raw_instruction(config.raw_instructions[0], config.code_page)
    } else {
        crate::Program::from_raw_instructions(config.raw_instructions)
    };
    let stack = Box::new(Stack::with_read(
        config.stack.read_value,
        config.stack.read_is_pointer,
    ));
    let world_diff = WorldDiff::default();
    let world_before_this_frame = world_diff.snapshot();
    let mut current_frame = Callframe::new(
        config.frame.address,
        config.frame.code_address,
        config.frame.caller,
        program,
        stack,
        config.frame.heap,
        config.frame.aux_heap,
        config.frame.calldata_heap,
        config.frame.gas,
        config.frame.exception_handler,
        config.frame.context_u128,
        config.frame.is_static,
        false,
        world_before_this_frame,
    );
    current_frame.sp = config.frame.sp;
    current_frame.heap_size = config.frame.heap_bound;
    current_frame.aux_heap_size = config.frame.aux_heap_bound;

    let mut registers = [U256::zero(); 16];
    for (index, (value, _)) in config.registers.iter().copied().enumerate().skip(1) {
        registers[index] = value;
    }
    let register_pointer_flags = config
        .registers
        .iter()
        .enumerate()
        .fold(0_u16, |flags, (index, (_, is_pointer))| {
            flags | (u16::from(*is_pointer) << index)
        });

    let mut previous_frames = Vec::new();
    if config.include_parent_frame {
        previous_frames.push(Callframe::dummy());
    }

    let vm = VirtualMachine {
        world_diff,
        state: State {
            registers,
            register_pointer_flags,
            flags: Flags::new(config.flags.0, config.flags.1, config.flags.2),
            current_frame,
            previous_frames,
            heaps: heap::Heaps::with_read(
                config.memory.heap_id,
                heap::Heap::with_read_u256(config.memory.heap_read_u256),
            ),
            transaction_number: config.transaction_number,
            context_u128: config.context_u128,
            next_base_page: config.frame.base_page,
        },
        settings: config.settings,
        stack_pool: Default::default(),
        snapshot: None,
    };
    let world = MockWorld::with_storage_read(config.storage_read);
    (vm, world)
}
