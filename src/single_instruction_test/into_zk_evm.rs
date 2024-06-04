use std::sync::Arc;

use super::{stack::Stack, state_to_zk_evm::vm2_state_to_zk_evm_state, MockWorld};
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
        memory: MockMemory {
            code_page: vm.state.current_frame.program.code_page().clone(),
            stack: *vm.state.current_frame.stack.clone(),
        },
        event_sink: InMemoryEventSink::new(),
        precompiles_processor: NoOracle,
        decommittment_processor: MockDecommitter,
        witness_tracer: NoOracle,
    }
}

#[derive(Debug)]
pub struct MockMemory {
    code_page: Arc<[U256]>,
    stack: Stack,
}

impl Memory for MockMemory {
    fn execute_partial_query(
        &mut self,
        _: u32,
        mut query: zk_evm::aux_structures::MemoryQuery,
    ) -> zk_evm::aux_structures::MemoryQuery {
        match query.location.memory_type {
            zk_evm::abstractions::MemoryType::Stack => {
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
            .cloned()
            .unwrap_or_default();
        query
    }
}

#[derive(Debug)]
pub struct MockWorldWrapper(MockWorld);

impl Storage for MockWorldWrapper {
    fn get_access_refund(
        &mut self, // to avoid any hacks inside, like prefetch
        _: u32,
        _partial_query: &zk_evm::aux_structures::LogQuery,
    ) -> zk_evm::abstractions::StorageAccessRefund {
        todo!()
    }

    fn execute_partial_query(
        &mut self,
        _: u32,
        _query: zk_evm::aux_structures::LogQuery,
    ) -> (
        zk_evm::aux_structures::LogQuery,
        zk_evm::aux_structures::PubdataCost,
    ) {
        todo!()
    }

    fn start_frame(&mut self, _: zk_evm::aux_structures::Timestamp) {
        todo!()
    }

    fn finish_frame(&mut self, _: zk_evm::aux_structures::Timestamp, _panicked: bool) {
        todo!()
    }

    fn start_new_tx(&mut self, _: zk_evm::aux_structures::Timestamp) {
        todo!()
    }
}

#[derive(Debug)]
pub struct MockDecommitter;

impl DecommittmentProcessor for MockDecommitter {
    fn prepare_to_decommit(
        &mut self,
        _: u32,
        _partial_query: zk_evm::aux_structures::DecommittmentQuery,
    ) -> anyhow::Result<zk_evm::aux_structures::DecommittmentQuery> {
        todo!()
    }

    fn decommit_into_memory<M: zk_evm::abstractions::Memory>(
        &mut self,
        _: u32,
        _partial_query: zk_evm::aux_structures::DecommittmentQuery,
        _memory: &mut M,
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
