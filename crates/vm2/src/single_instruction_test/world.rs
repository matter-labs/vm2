use arbitrary::Arbitrary;
use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS, SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
};
use zksync_vm2_interface::Tracer;

use super::mock_array::MockRead;
use crate::{
    precompiles::{PrecompileMemoryReader, PrecompileOutput, Precompiles},
    Program, StorageInterface, StorageSlot, World,
};

#[derive(Debug)]
struct MockPrecompiles;

impl Precompiles for MockPrecompiles {
    fn call_precompile(
        &self,
        address_low: u16,
        memory: PrecompileMemoryReader<'_>,
        _: u64,
    ) -> PrecompileOutput {
        match address_low {
            SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS if memory.len() != 0 => {
                let _ = memory.assume_offset_in_words().next();
            }
            KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS if memory.len() != 0 => {
                let mut input_byte_offset = memory.offset() as usize;
                let mut bytes_left = memory.len_u32() as usize;
                let mut reads = 0;
                while bytes_left != 0 && reads < 2 {
                    let memory_index = input_byte_offset / 32;
                    let unalignment = input_byte_offset % 32;
                    let bytes_in_query = bytes_left.min(32 - unalignment);
                    let _ = memory.read_u256_by_word_index(memory_index as u32);
                    input_byte_offset += bytes_in_query;
                    bytes_left -= bytes_in_query;
                    reads += 1;
                }
            }
            _ => {}
        }
        [U256::zero(), U256::zero()].into()
    }
}

static MOCK_PRECOMPILES: MockPrecompiles = MockPrecompiles;

#[derive(Debug, Arbitrary, Clone)]
pub struct MockWorld {
    storage_slot: MockRead<(H160, U256), Option<U256>>,
    storage_write_cost: u32,
}

impl<T: Tracer> World<T> for MockWorld {
    fn decommit(&mut self, _hash: U256) -> Program<T, Self> {
        Program::for_decommit()
    }

    fn decommit_code(&mut self, _hash: U256) -> Vec<u8> {
        vec![0; 32]
    }

    fn precompiles(&self) -> &impl Precompiles {
        &MOCK_PRECOMPILES
    }
}

impl MockWorld {
    pub fn with_storage_read(value: Option<U256>) -> Self {
        Self::with_storage_read_and_write_cost(value, 50)
    }

    pub fn with_storage_read_and_write_cost(value: Option<U256>, storage_write_cost: u32) -> Self {
        Self {
            storage_slot: MockRead::new(value),
            storage_write_cost,
        }
    }

    pub(crate) fn storage_write_cost(&self) -> u32 {
        self.storage_write_cost
    }
}

impl StorageInterface for MockWorld {
    fn read_storage(&mut self, contract: H160, key: U256) -> StorageSlot {
        let value = *self.storage_slot.get((contract, key));
        StorageSlot {
            value: value.unwrap_or_default(),
            is_write_initial: value.is_none(),
        }
    }

    fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
        self.storage_write_cost
    }

    fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
        false
    }
}
