use crate::{program::Program, world_diff::WorldDiff, World};
use u256::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

impl WorldDiff {
    pub(crate) fn decommit(
        &mut self,
        world: &mut dyn World,
        address: U256,
        default_aa_code_hash: [u8; 32],
        evm_interpreter_code_hash: [u8; 32],
        gas: &mut u32,
        is_constructor_call: bool,
    ) -> Option<(Program, bool)> {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

        let mut is_evm = false;

        let mut code_info = {
            let (code_info, _) =
                self.read_storage(world, deployer_system_contract_address, address);
            let mut code_info_bytes = [0; 32];
            code_info.to_big_endian(&mut code_info_bytes);

            // Note that EOAs are considered constructed because their code info is all zeroes.
            let is_constructed = match code_info_bytes[1] {
                0 => true,
                1 => false,
                _ => {
                    return None;
                }
            };

            // The address aliasing contract implements Ethereum-like behavior of calls to EOAs
            // returning successfully (and address aliasing when called from the bootloader).
            // It makes sense that unconstructed code is treated as an EOA but for some reason
            // a constructor call to constructed code is also treated as EOA.
            if (is_constructed == is_constructor_call || code_info == U256::zero())
                && !is_kernel(u256_into_address(address))
            {
                default_aa_code_hash
            } else {
                match code_info_bytes[0] {
                    1 => code_info_bytes,
                    2 => {
                        is_evm = true;
                        evm_interpreter_code_hash
                    }
                    _ => return None,
                }
            }
        };

        code_info[1] = 0;
        let code_key: U256 = U256::from_big_endian(&code_info);

        if !self.decommitted_hashes.as_ref().contains_key(&code_key) {
            let code_length_in_words = u16::from_be_bytes([code_info[2], code_info[3]]);
            let cost =
                code_length_in_words as u32 * zkevm_opcode_defs::ERGS_PER_CODE_WORD_DECOMMITTMENT;
            if cost > *gas {
                // Unlike all other gas costs, this one is not paid if low on gas.
                return None;
            }
            *gas -= cost;
            self.decommitted_hashes.insert(code_key, ());
        };

        let program = world.decommit(code_key);
        Some((program, is_evm))
    }
}

/// May be used to load code when the VM first starts up.
/// Doesn't check for any errors.
/// Doesn't cost anything but also doesn't make the code free in future decommits.
pub fn initial_decommit(world: &mut impl World, address: H160) -> Program {
    let deployer_system_contract_address =
        Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);
    let code_info = world
        .read_storage(deployer_system_contract_address, address_into_u256(address))
        .unwrap_or_default();

    let mut code_info_bytes = [0; 32];
    code_info.to_big_endian(&mut code_info_bytes);

    code_info_bytes[1] = 0;
    let code_key: U256 = U256::from_big_endian(&code_info_bytes);

    world.decommit(code_key)
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

pub(crate) fn is_kernel(address: H160) -> bool {
    address.0[..18].iter().all(|&byte| byte == 0)
}
