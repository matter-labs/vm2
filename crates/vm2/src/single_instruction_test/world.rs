use arbitrary::Arbitrary;
use primitive_types::{H160, U256};
use zksync_vm2_interface::Tracer;

use super::mock_array::MockRead;
use crate::{Program, StorageInterface, World};

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
}

impl StorageInterface for MockWorld {
    fn read_storage(&mut self, contract: H160, key: U256) -> Option<U256> {
        *self.storage_slot.get((contract, key))
    }

    fn cost_of_writing_storage(&mut self, _: Option<U256>, _: U256) -> u32 {
        50
    }

    fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
        false
    }
}
