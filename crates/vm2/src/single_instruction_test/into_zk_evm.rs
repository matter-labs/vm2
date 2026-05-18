use std::{collections::BTreeMap, sync::Arc};

use primitive_types::{H160, U256};
use zk_evm::{
    abstractions::{DecommittmentProcessor, Memory, MemoryType, PrecompilesProcessor, Storage},
    aux_structures::PubdataCost,
    block_properties::BlockProperties,
    tracing,
    vm_state::{Version, VmState},
    witness_trace::VmWitnessTracer,
};
use zk_evm_abstractions::vm::EventSink;
use zkevm_opcode_defs::{
    decoding::EncodingModeProduction,
    system_params::{
        PRECOMPILE_AUX_BYTE, STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST,
        STORAGE_ACCESS_WARM_READ_COST, STORAGE_ACCESS_WARM_WRITE_COST,
    },
    PrecompileCallABI, KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
    SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS, TRANSIENT_STORAGE_AUX_BYTE,
};
use zksync_vm2_interface::Tracer;

use super::{stack::Stack, state_to_zk_evm::vm2_state_to_zk_evm_state, MockWorld};
use crate::{StorageInterface, VirtualMachine, World};

type ZkEvmState = VmState<
    MockWorldWrapper,
    MockMemory,
    MockEventSink,
    NoOracle,
    MockDecommitter,
    NoOracle,
    8,
    EncodingModeProduction,
>;

pub fn vm2_to_zk_evm<T: Tracer, W: World<T>>(
    vm: &VirtualMachine<T, W>,
    world: MockWorld,
) -> ZkEvmState {
    let mut event_sink = MockEventSink::new();
    event_sink.start_frame(zk_evm::aux_structures::Timestamp(0));

    VmState {
        local_state: vm2_state_to_zk_evm_state(&vm.state),
        block_properties: BlockProperties {
            default_aa_code_hash: U256::from_big_endian(&vm.settings.default_aa_code_hash),
            evm_emulator_code_hash: U256::from_big_endian(&vm.settings.evm_interpreter_code_hash),
            zkporter_is_available: false,
        },
        storage: MockWorldWrapper::new(world),
        memory: MockMemory {
            code_page: vm.state.current_frame.program.code_page().clone(),
            stack: *vm.state.current_frame.stack.clone(),
            heap_read: None,
            heap_write: None,
        },
        event_sink,
        precompiles_processor: NoOracle::default(),
        decommittment_processor: MockDecommitter,
        witness_tracer: NoOracle::default(),
        version: Version::Version27,
    }
}

pub fn add_heap_to_zk_evm<T, W>(
    zk_evm: &mut ZkEvmState,
    vm_after_execution: &VirtualMachine<T, W>,
) {
    if let Some((heapid, heap)) = vm_after_execution.state.heaps.read.read_that_happened() {
        if let Some((start_index, mut value)) = heap.read.read_that_happened() {
            value.reverse();

            zk_evm.memory.heap_read = Some(ExpectedHeapValue {
                heap: heapid.as_u32(),
                start_index,
                value,
            });
        }
        if let Some((start_index, value_u256)) = heap.write {
            let mut value = [0; 32];
            value_u256.to_big_endian(&mut value);

            zk_evm.memory.heap_write = Some(ExpectedHeapValue {
                heap: heapid.as_u32(),
                start_index,
                value,
            });
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MockEventFrame {
    pub forward: Vec<zk_evm::aux_structures::LogQuery>,
    pub rollbacks: Vec<zk_evm::aux_structures::LogQuery>,
}

#[derive(Clone, Debug)]
pub struct MockEventSink {
    pub frames_stack: Vec<MockEventFrame>,
}

impl MockEventSink {
    fn new() -> Self {
        Self {
            frames_stack: vec![MockEventFrame::default()],
        }
    }
}

impl EventSink for MockEventSink {
    fn add_partial_query(&mut self, _: u32, mut query: zk_evm::aux_structures::LogQuery) {
        let frame = self.frames_stack.last_mut().expect("frame must be started");
        frame.forward.push(query);
        query.rollback = true;
        frame.rollbacks.push(query);
    }

    fn start_frame(&mut self, _: zk_evm::aux_structures::Timestamp) {
        self.frames_stack.push(MockEventFrame::default());
    }

    fn finish_frame(&mut self, panicked: bool, _: zk_evm::aux_structures::Timestamp) {
        let current = self.frames_stack.pop().unwrap_or_default();
        if let Some(parent) = self.frames_stack.last_mut() {
            parent.forward.extend(current.forward);
            if panicked {
                parent.forward.extend(current.rollbacks.into_iter().rev());
            } else {
                parent.rollbacks.extend(current.rollbacks);
            }
        } else {
            self.frames_stack.push(current);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MockStorageFrame {
    forward: Vec<zk_evm::aux_structures::LogQuery>,
    rollbacks: Vec<zk_evm::aux_structures::LogQuery>,
}

#[derive(Debug)]
pub struct MockMemory {
    code_page: Arc<[U256]>,
    stack: Stack,
    heap_read: Option<ExpectedHeapValue>,
    heap_write: Option<ExpectedHeapValue>,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct ExpectedHeapValue {
    heap: u32,
    start_index: u32,
    value: [u8; 32],
}

impl ExpectedHeapValue {
    /// Returns a new U256 that contains data from the heap value and zero elsewhere.
    /// One arbitrary heap value is not enough for `zk_evm` because it reads two U256s to read one U256.
    fn partially_overlapping_u256(&self, start: u32) -> U256 {
        let mut read = [0; 32];
        for i in 0..32 {
            if start + i >= self.start_index {
                let j = start + i - self.start_index;
                if j < 32 {
                    read[i as usize] = self.value[j as usize];
                }
            }
        }
        U256::from_big_endian(&read)
    }
}

impl Memory for MockMemory {
    fn execute_partial_query(
        &mut self,
        _: u32,
        mut query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        match query.location.memory_type {
            MemoryType::Stack => {
                #[allow(clippy::cast_possible_truncation)] // intentional
                let slot = query.location.index.0 as u16;
                if query.rw_flag {
                    self.stack.set(slot, query.value);
                    if query.value_is_pointer {
                        self.stack.set_pointer_flag(slot);
                    } else {
                        self.stack.clear_pointer_flag(slot);
                    }
                } else {
                    query.value = self.stack.get(slot);
                    query.value_is_pointer = self.stack.get_pointer_flag(slot);
                }
                query
            }
            MemoryType::Heap
            | MemoryType::AuxHeap
            | MemoryType::FatPointer
            | MemoryType::StaticMemory => {
                if query.rw_flag {
                    let heap = self.heap_write.unwrap();
                    assert_eq!(heap.heap, query.location.page.0);

                    assert_eq!(
                        query.value,
                        heap.partially_overlapping_u256(query.location.index.0 * 32)
                    );
                } else if let Some(heap) = self.heap_read {
                    // ^ Writes read the heap too, but they are just fed zeroes

                    assert_eq!(query.location.page.0, heap.heap);

                    query.value = query
                        .location
                        .index
                        .0
                        .checked_mul(32)
                        .map_or_else(U256::zero, |start| heap.partially_overlapping_u256(start));
                }
                query
            }
            _ => todo!(),
        }
    }

    fn specialized_code_query(
        &mut self,
        _: u32,
        _query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        todo!()
    }

    fn read_code_query(
        &self,
        _: u32,
        mut query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        // Code page read, instruction reads don't happen because the code word cache has been set up
        query.value = self
            .code_page
            .get(query.location.index.0 as usize)
            .copied()
            .unwrap_or_default();
        query
    }
}

#[derive(Debug)]
pub struct MockWorldWrapper {
    world: MockWorld,
    storage_changes: BTreeMap<(H160, U256), U256>,
    paid_storage_costs: BTreeMap<(H160, U256), u32>,
    read_storage_slots: BTreeMap<(H160, U256), ()>,
    written_storage_slots: BTreeMap<(H160, U256), ()>,
    storage_frames_stack: Vec<MockStorageFrame>,
    transient_storage: BTreeMap<(H160, U256), U256>,
    transient_logs: Vec<zk_evm::aux_structures::LogQuery>,
}

impl MockWorldWrapper {
    pub(crate) fn new(world: MockWorld) -> Self {
        Self {
            world,
            storage_changes: BTreeMap::new(),
            paid_storage_costs: BTreeMap::new(),
            read_storage_slots: BTreeMap::new(),
            written_storage_slots: BTreeMap::new(),
            storage_frames_stack: vec![MockStorageFrame::default()],
            transient_storage: BTreeMap::new(),
            transient_logs: Vec::new(),
        }
    }

    pub(crate) fn transient_logs(&self) -> &[zk_evm::aux_structures::LogQuery] {
        &self.transient_logs
    }

    pub(crate) fn storage_logs(&self) -> Vec<zk_evm::aux_structures::LogQuery> {
        self.storage_frames_stack
            .iter()
            .flat_map(|frame| frame.forward.iter().copied())
            .collect()
    }

    fn read_storage_value(&mut self, address: H160, key: U256) -> U256 {
        self.storage_changes
            .get(&(address, key))
            .copied()
            .unwrap_or_else(|| self.world.read_storage_value(address, key))
    }
}

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;
const MOCK_STORAGE_WRITE_COST: u32 = 50;

impl Storage for MockWorldWrapper {
    fn get_access_refund(
        &mut self, // to avoid any hacks inside, like prefetch
        _: u32,
        partial_query: &zk_evm::aux_structures::LogQuery,
    ) -> zk_evm::abstractions::StorageAccessRefund {
        if partial_query.aux_byte == TRANSIENT_STORAGE_AUX_BYTE {
            return zk_evm::abstractions::StorageAccessRefund::Cold;
        }

        let key = (partial_query.address, partial_query.key);
        let refund = if partial_query.rw_flag {
            if self.written_storage_slots.contains_key(&key) {
                WARM_WRITE_REFUND
            } else if self.read_storage_slots.contains_key(&key) {
                COLD_WRITE_AFTER_WARM_READ_REFUND
            } else {
                0
            }
        } else if self.read_storage_slots.contains_key(&key) {
            WARM_READ_REFUND
        } else {
            0
        };

        if refund == 0 {
            zk_evm::abstractions::StorageAccessRefund::Cold
        } else {
            zk_evm::abstractions::StorageAccessRefund::Warm { ergs: refund }
        }
    }

    fn execute_partial_query(
        &mut self,
        _: u32,
        mut query: zk_evm::aux_structures::LogQuery,
    ) -> (zk_evm::aux_structures::LogQuery, PubdataCost) {
        let is_transient = query.aux_byte == TRANSIENT_STORAGE_AUX_BYTE;
        if query.rw_flag {
            if is_transient {
                query.read_value = self
                    .transient_storage
                    .get(&(query.address, query.key))
                    .copied()
                    .unwrap_or_default();
                self.transient_storage
                    .insert((query.address, query.key), query.written_value);
                self.transient_logs.push(query);
                (query, PubdataCost(0))
            } else {
                let key = (query.address, query.key);
                query.read_value = self.read_storage_value(query.address, query.key);
                self.storage_changes.insert(key, query.written_value);
                self.read_storage_slots.insert(key, ());
                self.written_storage_slots.insert(key, ());
                let mut rollback = query;
                rollback.rollback = true;
                let frame = self
                    .storage_frames_stack
                    .last_mut()
                    .expect("storage frame must exist");
                frame.forward.push(query);
                frame.rollbacks.push(rollback);
                let prepaid = self
                    .paid_storage_costs
                    .insert(key, MOCK_STORAGE_WRITE_COST)
                    .unwrap_or_default();
                (
                    query,
                    PubdataCost(MOCK_STORAGE_WRITE_COST as i32 - prepaid as i32),
                )
            }
        } else {
            query.read_value = if is_transient {
                self.transient_storage
                    .get(&(query.address, query.key))
                    .copied()
                    .unwrap_or_default()
            } else {
                self.read_storage_value(query.address, query.key)
            };
            if is_transient {
                self.transient_logs.push(query);
            } else {
                self.read_storage_slots
                    .insert((query.address, query.key), ());
                self.storage_frames_stack
                    .last_mut()
                    .expect("storage frame must exist")
                    .forward
                    .push(query);
            }
            (query, PubdataCost(0))
        }
    }

    fn start_frame(&mut self, _: zk_evm::aux_structures::Timestamp) {
        self.storage_frames_stack.push(MockStorageFrame::default());
    }

    fn finish_frame(&mut self, _: zk_evm::aux_structures::Timestamp, panicked: bool) {
        let current = self
            .storage_frames_stack
            .pop()
            .expect("storage frame must exist");
        if let Some(parent) = self.storage_frames_stack.last_mut() {
            if panicked {
                for query in current.rollbacks.iter().rev() {
                    self.storage_changes
                        .insert((query.address, query.key), query.read_value);
                }
                parent.forward.extend(current.forward);
                parent.forward.extend(current.rollbacks.into_iter().rev());
            } else {
                parent.forward.extend(current.forward);
                parent.rollbacks.extend(current.rollbacks);
            }
        } else {
            self.storage_frames_stack.push(current);
        }
    }

    fn start_new_tx(&mut self, _: zk_evm::aux_structures::Timestamp) {
        self.transient_storage.clear();
    }
}

#[derive(Debug)]
pub struct MockDecommitter;

impl DecommittmentProcessor for MockDecommitter {
    fn prepare_to_decommit(
        &mut self,
        _: u32,
        mut partial_query: zk_evm::aux_structures::DecommittmentQuery,
    ) -> anyhow::Result<zk_evm::aux_structures::DecommittmentQuery> {
        partial_query.is_fresh = true;
        Ok(partial_query)
    }

    fn decommit_into_memory<M: Memory>(
        &mut self,
        _: u32,
        _partial_query: zk_evm::aux_structures::DecommittmentQuery,
        _memory: &mut M,
    ) -> anyhow::Result<Option<Vec<U256>>> {
        Ok(None)
    }
}

#[derive(Debug)]
pub struct NoTracer;

impl tracing::Tracer for NoTracer {
    type SupportedMemory = MockMemory;

    fn before_decoding(
        &mut self,
        _: zk_evm::tracing::VmLocalStateData<'_, 8, EncodingModeProduction>,
        _: &Self::SupportedMemory,
    ) {
    }

    fn after_decoding(
        &mut self,
        _: zk_evm::tracing::VmLocalStateData<'_, 8, EncodingModeProduction>,
        _: zk_evm::tracing::AfterDecodingData<8, EncodingModeProduction>,
        _: &Self::SupportedMemory,
    ) {
    }

    fn before_execution(
        &mut self,
        _: zk_evm::tracing::VmLocalStateData<'_, 8, EncodingModeProduction>,
        _: zk_evm::tracing::BeforeExecutionData<8, EncodingModeProduction>,
        _: &Self::SupportedMemory,
    ) {
    }

    fn after_execution(
        &mut self,
        _: zk_evm::tracing::VmLocalStateData<'_, 8, EncodingModeProduction>,
        _: zk_evm::tracing::AfterExecutionData<8, EncodingModeProduction>,
        _: &Self::SupportedMemory,
    ) {
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoOracle {
    precompile_logs: Vec<zk_evm::aux_structures::LogQuery>,
}

impl NoOracle {
    pub(crate) fn precompile_logs(&self) -> &[zk_evm::aux_structures::LogQuery] {
        &self.precompile_logs
    }
}

impl PrecompilesProcessor for NoOracle {
    fn execute_precompile<M: Memory>(
        &mut self,
        monotonic_cycle_counter: u32,
        query: zk_evm::aux_structures::LogQuery,
        memory: &mut M,
    ) -> Option<(
        Vec<zk_evm::aux_structures::MemoryQuery>,
        Vec<zk_evm::aux_structures::MemoryQuery>,
        zk_evm::abstractions::PrecompileCyclesWitness,
    )> {
        let address_bytes = query.address.0;
        let address_low = u16::from_le_bytes([address_bytes[19], address_bytes[18]]);
        let abi = PrecompileCallABI::from_u256(query.key);
        match address_low {
            SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                let memory_query = zk_evm::aux_structures::MemoryQuery {
                    timestamp: query.timestamp,
                    location: zk_evm::aux_structures::MemoryLocation {
                        memory_type: MemoryType::Heap,
                        page: zk_evm::aux_structures::MemoryPage(abi.memory_page_to_read),
                        index: zk_evm::aux_structures::MemoryIndex(abi.input_memory_offset),
                    },
                    value: U256::zero(),
                    value_is_pointer: false,
                    rw_flag: false,
                };
                let _ = memory.execute_partial_query(monotonic_cycle_counter, memory_query);
            }
            KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                let mut input_byte_offset = abi.input_memory_offset as usize;
                let mut bytes_left = abi.input_memory_length as usize;
                let mut reads = 0;
                while bytes_left != 0 && reads < 2 {
                    let memory_index = input_byte_offset / 32;
                    let unalignment = input_byte_offset % 32;
                    let bytes_in_query = bytes_left.min(32 - unalignment);
                    let memory_query = zk_evm::aux_structures::MemoryQuery {
                        timestamp: query.timestamp,
                        location: zk_evm::aux_structures::MemoryLocation {
                            memory_type: MemoryType::FatPointer,
                            page: zk_evm::aux_structures::MemoryPage(abi.memory_page_to_read),
                            index: zk_evm::aux_structures::MemoryIndex(memory_index as u32),
                        },
                        value: U256::zero(),
                        value_is_pointer: false,
                        rw_flag: false,
                    };
                    let _ = memory.execute_partial_query(monotonic_cycle_counter, memory_query);
                    input_byte_offset += bytes_in_query;
                    bytes_left -= bytes_in_query;
                    reads += 1;
                }
            }
            _ => return None,
        }
        None
    }

    fn start_frame(&mut self) {}

    fn finish_frame(&mut self, _: bool) {}
}

impl VmWitnessTracer<8, EncodingModeProduction> for NoOracle {
    fn start_new_execution_cycle(
        &mut self,
        _: &zk_evm::vm_state::VmLocalState<8, EncodingModeProduction>,
    ) {
    }

    fn end_execution_cycle(
        &mut self,
        _: &zk_evm::vm_state::VmLocalState<8, EncodingModeProduction>,
    ) {
    }

    fn add_memory_query(&mut self, _: u32, _: zk_evm::aux_structures::MemoryQuery) {}

    fn record_refund_for_query(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::LogQuery,
        _: zk_evm::abstractions::StorageAccessRefund,
    ) {
    }

    fn add_log_query(&mut self, _: u32, query: zk_evm::aux_structures::LogQuery) {
        if query.aux_byte == PRECOMPILE_AUX_BYTE {
            self.precompile_logs.push(query);
        }
    }

    fn record_pubdata_cost_for_query(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::LogQuery,
        _: PubdataCost,
    ) {
    }

    fn prepare_for_decommittment(&mut self, _: u32, _: zk_evm::aux_structures::DecommittmentQuery) {
    }

    fn execute_decommittment(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::DecommittmentQuery,
        _: Vec<U256>,
    ) {
    }

    fn add_precompile_call_result(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::LogQuery,
        _: Vec<zk_evm::aux_structures::MemoryQuery>,
        _: Vec<zk_evm::aux_structures::MemoryQuery>,
        _: zk_evm::abstractions::PrecompileCyclesWitness,
    ) {
    }

    fn add_revertable_precompile_call(&mut self, _: u32, _: zk_evm::aux_structures::LogQuery) {}

    fn start_new_execution_context(
        &mut self,
        _: u32,
        _: &zk_evm::vm_state::CallStackEntry<8, EncodingModeProduction>,
        _: &zk_evm::vm_state::CallStackEntry<8, EncodingModeProduction>,
    ) {
    }

    fn finish_execution_context(&mut self, _: u32, _: bool) {}
}
