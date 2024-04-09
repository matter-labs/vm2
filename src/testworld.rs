use crate::{address_into_u256, Program, World};
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap},
    hash::{Hash, Hasher},
};
use u256::U256;
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

pub struct TestWorld {
    pub address_to_hash: BTreeMap<U256, U256>,
    pub hash_to_contract: BTreeMap<U256, Program>,
}

impl TestWorld {
    pub fn new(contracts: &[(Address, Program)]) -> Self {
        let mut address_to_hash = BTreeMap::new();
        let mut hash_to_contract = BTreeMap::new();
        for (address, code) in contracts {
            // The hash is actually computed from the code page but tests may leave it blank, so let's not.
            let mut hasher = DefaultHasher::new();
            code.instructions().hash(&mut hasher);
            code.code_page().hash(&mut hasher);

            let mut code_info_bytes = [0; 32];
            code_info_bytes[24..].copy_from_slice(&hasher.finish().to_be_bytes());
            code_info_bytes[2..=3].copy_from_slice(&(code.code_page().len() as u16).to_be_bytes());
            code_info_bytes[0] = 1;
            let hash = U256::from_big_endian(&code_info_bytes);

            address_to_hash.insert(address_into_u256(*address), hash);
            hash_to_contract.insert(hash, code.clone());
        }
        Self {
            address_to_hash,
            hash_to_contract,
        }
    }
}

impl World for TestWorld {
    fn decommit(&mut self, hash: u256::U256) -> Program {
        if let Some(program) = self.hash_to_contract.get(&hash) {
            program.clone()
        } else {
            panic!("unexpected decommit")
        }
    }

    fn read_storage(&mut self, contract: u256::H160, key: u256::U256) -> u256::U256 {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

        if contract == deployer_system_contract_address {
            self.address_to_hash
                .get(&key)
                .copied()
                .unwrap_or_else(|| U256::zero())
        } else {
            0.into()
        }
    }

    fn handle_hook(&mut self, _: u32, _: &mut crate::State) {
        unimplemented!()
    }
}
