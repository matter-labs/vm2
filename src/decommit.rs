use crate::{instruction::Panic, modified_world::ModifiedWorld, Instruction};
use std::sync::Arc;
use u256::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

impl ModifiedWorld {
    pub(crate) fn decommit(
        &mut self,
        address: U256,
        default_aa_code_hash: U256,
        gas: &mut u32,
        is_constructor_call: bool,
    ) -> Result<(Arc<[Instruction]>, Arc<[U256]>), Panic> {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

        let code_info = {
            let code_info = self.read_storage(deployer_system_contract_address, address);
            // The Ethereum-like behavior of calls to EOAs returning successfully is implemented
            // by the default address aliasing contract.
            // That contract also implements AA but only when called from the bootloader.
            if code_info == U256::zero() && !is_kernel(address) {
                default_aa_code_hash
            } else {
                code_info
            }
        };

        let mut code_info_bytes = [0; 32];
        code_info.to_big_endian(&mut code_info_bytes);

        if code_info_bytes[0] != 1 {
            return Err(Panic::MalformedCodeInfo);
        }
        let is_constructed = match code_info_bytes[1] {
            0 => true,
            1 => false,
            _ => {
                return Err(Panic::MalformedCodeInfo);
            }
        };
        if is_constructed == is_constructor_call {
            return Err(Panic::ConstructorCallAndCodeStatusMismatch);
        }

        let code_length_in_words = u16::from_be_bytes([code_info_bytes[2], code_info_bytes[3]]);

        code_info_bytes[1] = 0;
        let code_key: U256 = U256::from_big_endian(&code_info_bytes);

        if !self.decommitted_hashes.as_ref().contains_key(&code_key) {
            let cost =
                code_length_in_words as u32 * zkevm_opcode_defs::ERGS_PER_CODE_WORD_DECOMMITTMENT;
            if cost > *gas {
                // Unlike all other gas costs, this one is not paid if low on gas.
                return Err(Panic::OutOfGas);
            } else {
                *gas -= cost;
                self.decommitted_hashes.insert(code_key, ());
            }
        };

        Ok(self.world.decommit(code_key))
    }

    /// Used to load code when the VM first starts up.
    /// Doesn't check for any errors.
    /// Doesn't cost anything but also doesn't make the code free in future decommits.
    pub(crate) fn initial_decommit(&mut self, address: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);
        let code_info = self.read_storage(deployer_system_contract_address, address);

        let mut code_info_bytes = [0; 32];
        code_info.to_big_endian(&mut code_info_bytes);

        code_info_bytes[1] = 0;
        let code_key: U256 = U256::from_big_endian(&code_info_bytes);

        self.world.decommit(code_key)
    }
}

pub fn address_into_u256(address: H160) -> U256 {
    let mut buffer = [0; 32];
    buffer[12..].copy_from_slice(address.as_bytes());
    U256::from_big_endian(&buffer)
}

pub(crate) fn u256_into_address(source: U256) -> H160 {
    let mut result = H160::zero();
    let mut bytes = [0; 32];
    source.to_big_endian(&mut bytes);
    result.assign_from_slice(&bytes[12..]);
    result
}

pub(crate) fn is_kernel(address: U256) -> bool {
    address < (1 << 16).into()
}
