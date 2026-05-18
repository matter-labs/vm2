use arbitrary::Arbitrary;
use primitive_types::{H160, U256};
use zksync_vm2_interface::Tracer;

use super::mock_array::MockRead;
use crate::{
    precompiles::{PrecompileMemoryReader, PrecompileOutput, Precompiles},
    Program, StorageInterface, StorageSlot, World,
};

#[derive(Debug)]
struct MockPrecompiles;

impl Precompiles for MockPrecompiles {
    fn call_precompile(&self, _: u16, _: PrecompileMemoryReader<'_>, _: u64) -> PrecompileOutput {
        [U256::zero(), U256::zero()].into()
    }
}

static MOCK_PRECOMPILES: MockPrecompiles = MockPrecompiles;

#[derive(Debug, Arbitrary, Clone)]
pub struct MockWorld {
    storage_slot: MockRead<(H160, U256), Option<U256>>,
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
        Self {
            storage_slot: MockRead::new(value),
        }
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
        50
    }

    fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
        false
    }
}
