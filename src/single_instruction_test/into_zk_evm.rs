use super::{
    state_to_zk_evm::{vm2_state_to_zk_evm_state, zk_evm_state_equal},
    MockWorld,
};
use crate::{zkevm_opcode_defs::decoding::EncodingModeProduction, VirtualMachine};
use u256::U256;
use zk_evm::{
    abstractions::{DecommittmentProcessor, Memory, PrecompilesProcessor, Storage},
    block_properties::BlockProperties,
    reference_impls::event_sink::InMemoryEventSink,
    tracing::Tracer,
    vm_state::VmState,
    witness_trace::VmWitnessTracer,
};

pub fn vm2_to_zk_evm(
    vm: &VirtualMachine,
    world: MockWorld,
) -> VmState<
    MockWorldWrapper,
    MockMemory,
    InMemoryEventSink,
    NoOracle,
    MockDecommitter,
    NoOracle,
    8,
    EncodingModeProduction,
> {
    VmState {
        local_state: vm2_state_to_zk_evm_state(&vm.state),
        block_properties: BlockProperties {
            default_aa_code_hash: U256::from_big_endian(&vm.settings.default_aa_code_hash),
            evm_simulator_code_hash: U256::from_big_endian(&vm.settings.evm_interpreter_code_hash),
            zkporter_is_available: false,
        },
        storage: MockWorldWrapper(world),
        memory: MockMemory,
        event_sink: InMemoryEventSink::new(),
        precompiles_processor: NoOracle,
        decommittment_processor: MockDecommitter,
        witness_tracer: NoOracle,
    }
}

pub fn zk_evm_equal(
    vm1: &VmState<
        MockWorldWrapper,
        MockMemory,
        InMemoryEventSink,
        NoOracle,
        MockDecommitter,
        NoOracle,
        8,
        EncodingModeProduction,
    >,
    vm2: &VmState<
        MockWorldWrapper,
        MockMemory,
        InMemoryEventSink,
        NoOracle,
        MockDecommitter,
        NoOracle,
        8,
        EncodingModeProduction,
    >,
) -> bool {
    zk_evm_state_equal(&vm1.local_state, &vm2.local_state)
}

#[derive(Debug)]
pub struct MockMemory;

impl Memory for MockMemory {
    fn execute_partial_query(
        &mut self,
        monotonic_cycle_counter: u32,
        query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        todo!()
    }

    fn specialized_code_query(
        &mut self,
        monotonic_cycle_counter: u32,
        query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        todo!()
    }

    fn read_code_query(
        &self,
        monotonic_cycle_counter: u32,
        query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        todo!()
    }
}

#[derive(Debug)]
pub struct MockWorldWrapper(MockWorld);

impl Storage for MockWorldWrapper {
    fn get_access_refund(
        &mut self, // to avoid any hacks inside, like prefetch
        monotonic_cycle_counter: u32,
        partial_query: &zk_evm::aux_structures::LogQuery,
    ) -> zk_evm::abstractions::StorageAccessRefund {
        todo!()
    }

    fn execute_partial_query(
        &mut self,
        monotonic_cycle_counter: u32,
        query: zk_evm::aux_structures::LogQuery,
    ) -> (
        zk_evm::aux_structures::LogQuery,
        zk_evm::aux_structures::PubdataCost,
    ) {
        todo!()
    }

    fn start_frame(&mut self, timestamp: zk_evm::aux_structures::Timestamp) {
        todo!()
    }

    fn finish_frame(&mut self, timestamp: zk_evm::aux_structures::Timestamp, panicked: bool) {
        todo!()
    }

    fn start_new_tx(&mut self, timestamp: zk_evm::aux_structures::Timestamp) {
        todo!()
    }
}

#[derive(Debug)]
pub struct MockDecommitter;

impl DecommittmentProcessor for MockDecommitter {
    fn prepare_to_decommit(
        &mut self,
        monotonic_cycle_counter: u32,
        partial_query: zk_evm::aux_structures::DecommittmentQuery,
    ) -> anyhow::Result<zk_evm::aux_structures::DecommittmentQuery> {
        todo!()
    }

    fn decommit_into_memory<M: zk_evm::abstractions::Memory>(
        &mut self,
        monotonic_cycle_counter: u32,
        partial_query: zk_evm::aux_structures::DecommittmentQuery,
        memory: &mut M,
    ) -> anyhow::Result<Option<Vec<zk_evm::ethereum_types::U256>>> {
        todo!()
    }
}

#[derive(Debug)]
pub struct NoTracer;

impl Tracer for NoTracer {
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

#[derive(Debug, Clone)]
pub struct NoOracle;

impl PrecompilesProcessor for NoOracle {
    fn execute_precompile<M: zk_evm::abstractions::Memory>(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::LogQuery,
        _: &mut M,
    ) -> Option<(
        Vec<zk_evm::aux_structures::MemoryQuery>,
        Vec<zk_evm::aux_structures::MemoryQuery>,
        zk_evm::abstractions::PrecompileCyclesWitness,
    )> {
        unimplemented!()
    }

    fn start_frame(&mut self) {
        unimplemented!()
    }

    fn finish_frame(&mut self, _: bool) {
        unimplemented!()
    }
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

    fn add_log_query(&mut self, _: u32, _: zk_evm::aux_structures::LogQuery) {}

    fn record_pubdata_cost_for_query(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::LogQuery,
        _: zk_evm::aux_structures::PubdataCost,
    ) {
    }

    fn prepare_for_decommittment(&mut self, _: u32, _: zk_evm::aux_structures::DecommittmentQuery) {
    }

    fn execute_decommittment(
        &mut self,
        _: u32,
        _: zk_evm::aux_structures::DecommittmentQuery,
        _: Vec<zk_evm::ethereum_types::U256>,
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
