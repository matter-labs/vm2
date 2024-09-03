use super::{common::boilerplate_t, HeapInterface};
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source},
    heap::Heaps,
    instruction::ExecutionStatus,
    Instruction, VirtualMachine,
};
use eravm_stable_interface::{opcodes, CycleStats, HeapId, Tracer};
use zk_evm_abstractions::{
    aux::Timestamp,
    precompiles::{
        ecrecover::ecrecover_function, keccak256::keccak256_rounds_function,
        secp256r1_verify::secp256r1_verify_function, sha256::sha256_rounds_function,
    },
    queries::LogQuery,
    vm::Memory,
};
use zkevm_opcode_defs::{
    system_params::{
        ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS, KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
        SECP256R1_VERIFY_PRECOMPILE_ADDRESS, SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
    },
    PrecompileAuxData, PrecompileCallABI,
};

fn precompile_call<T: Tracer, W>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate_t::<opcodes::PrecompileCall, _, _>(vm, world, tracer, |vm, args, tracer| {
        // The user gets to decide how much gas to burn
        // This is safe because system contracts are trusted
        let aux_data = PrecompileAuxData::from_u256(Register2::get(args, &mut vm.state));
        let Ok(()) = vm.state.use_gas(aux_data.extra_ergs_cost) else {
            vm.state.current_frame.pc = &*vm.panic;
            return;
        };
        vm.world_diff.pubdata.0 += aux_data.extra_pubdata_cost as i32;

        let mut abi = PrecompileCallABI::from_u256(Register1::get(args, &mut vm.state));
        if abi.memory_page_to_read == 0 {
            abi.memory_page_to_read = vm.state.current_frame.heap.to_u32();
        }
        if abi.memory_page_to_write == 0 {
            abi.memory_page_to_write = vm.state.current_frame.heap.to_u32();
        }

        let query = LogQuery {
            timestamp: Timestamp(0),
            key: abi.to_u256(),
            // only two first fields are read by the precompile
            tx_number_in_block: Default::default(),
            aux_byte: Default::default(),
            shard_id: Default::default(),
            address: Default::default(),
            read_value: Default::default(),
            written_value: Default::default(),
            rw_flag: Default::default(),
            rollback: Default::default(),
            is_service: Default::default(),
        };

        let address_bytes = vm.state.current_frame.address.0;
        let address_low = u16::from_le_bytes([address_bytes[19], address_bytes[18]]);
        let heaps = &mut vm.state.heaps;
        match address_low {
            KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                tracer.on_extra_prover_cycles(CycleStats::Keccak256(
                    keccak256_rounds_function::<_, false>(0, query, heaps).0 as u32,
                ));
            }
            SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                tracer.on_extra_prover_cycles(CycleStats::Sha256(
                    sha256_rounds_function::<_, false>(0, query, heaps).0 as u32,
                ));
            }
            ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS => {
                tracer.on_extra_prover_cycles(CycleStats::EcRecover(
                    ecrecover_function::<_, false>(0, query, heaps).0 as u32,
                ));
            }
            SECP256R1_VERIFY_PRECOMPILE_ADDRESS => {
                tracer.on_extra_prover_cycles(CycleStats::Secp256k1Verify(
                    secp256r1_verify_function::<_, false>(0, query, heaps).0 as u32,
                ));
            }
            _ => {
                // A precompile call may be used just to burn gas
            }
        }

        Register1::set(args, &mut vm.state, 1.into());
    })
}

impl Memory for Heaps {
    fn execute_partial_query(
        &mut self,
        _monotonic_cycle_counter: u32,
        mut query: zk_evm_abstractions::queries::MemoryQuery,
    ) -> zk_evm_abstractions::queries::MemoryQuery {
        let page = HeapId::from_u32_unchecked(query.location.page.0);

        let start = query.location.index.0 * 32;
        if query.rw_flag {
            self.write_u256(page, start, query.value);
        } else {
            query.value = self[page].read_u256(start);
            query.value_is_pointer = false;
        }
        query
    }

    fn specialized_code_query(
        &mut self,
        _monotonic_cycle_counter: u32,
        _query: zk_evm_abstractions::queries::MemoryQuery,
    ) -> zk_evm_abstractions::queries::MemoryQuery {
        todo!()
    }

    fn read_code_query(
        &self,
        _monotonic_cycle_counter: u32,
        _query: zk_evm_abstractions::queries::MemoryQuery,
    ) -> zk_evm_abstractions::queries::MemoryQuery {
        todo!()
    }
}

impl<T: Tracer, W> Instruction<T, W> {
    pub fn from_precompile_call(
        abi: Register1,
        burn: Register2,
        out: Register1,
        arguments: Arguments,
    ) -> Self {
        Self {
            arguments: arguments
                .write_source(&abi)
                .write_source(&burn)
                .write_destination(&out),
            handler: precompile_call,
        }
    }
}
