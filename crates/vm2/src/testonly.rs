//! Test-only tools for EraVM.

use std::{
    collections::{hash_map::DefaultHasher, BTreeMap},
    hash::{Hash, Hasher},
};

use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};
use zksync_vm2_interface::Tracer;

use crate::{instruction_handlers::address_into_u256, Program, StorageInterface, World};

/// Test [`World`] implementation.
#[derive(Debug)]
pub struct TestWorld<T> {
    pub(crate) address_to_hash: BTreeMap<U256, U256>,
    pub(crate) hash_to_contract: BTreeMap<U256, Program<T, Self>>,
}

impl<T: Tracer> TestWorld<T> {
    /// Creates a test world with the provided programs.
    ///
    /// # Panics
    ///
    /// Panics if the provided `Program`s are malformed.
    pub fn new(contracts: &[(Address, Program<T, Self>)]) -> Self {
        let mut address_to_hash = BTreeMap::new();
        let mut hash_to_contract = BTreeMap::new();
        for (i, (address, code)) in contracts.iter().enumerate() {
            // We add the index to the hash because tests may leave the code page blank.
            let mut hasher = DefaultHasher::new();
            i.hash(&mut hasher);
            code.code_page().hash(&mut hasher);

            let mut code_info_bytes = [0; 32];
            code_info_bytes[24..].copy_from_slice(&hasher.finish().to_be_bytes());
            let code_len = u16::try_from(code.code_page().len())
                .expect("code length must not exceed u16::MAX");
            code_info_bytes[2..=3].copy_from_slice(&code_len.to_be_bytes());
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

impl<T: Tracer> World<T> for TestWorld<T> {
    fn decommit(&mut self, hash: U256) -> Program<T, Self> {
        if let Some(program) = self.hash_to_contract.get(&hash) {
            program.clone()
        } else {
            panic!("unexpected decommit")
        }
    }

    fn decommit_code(&mut self, hash: U256) -> Vec<u8> {
        self.decommit(hash)
            .code_page()
            .iter()
            .flat_map(|u256| {
                let mut buffer = [0u8; 32];
                u256.to_big_endian(&mut buffer);
                buffer
            })
            .collect()
    }
}

impl<T> StorageInterface for TestWorld<T> {
    fn read_storage(&mut self, contract: H160, key: U256) -> Option<U256> {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW.into());

        if contract == deployer_system_contract_address {
            Some(
                self.address_to_hash
                    .get(&key)
                    .copied()
                    .unwrap_or(U256::zero()),
            )
        } else {
            None
        }
    }

    fn cost_of_writing_storage(&mut self, _initial_value: Option<U256>, _new_value: U256) -> u32 {
        50
    }

    fn is_free_storage_slot(&self, _contract: &H160, _key: &U256) -> bool {
        false
    }
}

/// May be used to load code when the VM first starts up.
/// Doesn't check for any errors.
/// Doesn't cost anything but also doesn't make the code free in future decommits.
#[doc(hidden)] // should be used only in low-level testing / benches
pub fn initial_decommit<T: Tracer, W: World<T>>(world: &mut W, address: H160) -> Program<T, W> {
    let deployer_system_contract_address =
        Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW.into());
    let code_info = world
        .read_storage(deployer_system_contract_address, address_into_u256(address))
        .unwrap_or_default();

    let mut code_info_bytes = [0; 32];
    code_info.to_big_endian(&mut code_info_bytes);

    code_info_bytes[1] = 0;
    let code_key: U256 = U256::from_big_endian(&code_info_bytes);

    world.decommit(code_key)
}
