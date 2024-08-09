use crate::{program::Program, world_diff::WorldDiff, CircuitCycleStatistic, World};
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
        is_constructor_call: bool,
    ) -> Option<(UnpaidDecommit, bool)> {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

        let mut is_evm = false;

        let mut code_info = {
            let code_info =
                self.read_storage_without_refund(world, deployer_system_contract_address, address);
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

            let try_default_aa = if is_kernel(u256_into_address(address)) {
                None
            } else {
                Some(default_aa_code_hash)
            };

            // The address aliasing contract implements Ethereum-like behavior of calls to EOAs
            // returning successfully (and address aliasing when called from the bootloader).
            // It makes sense that unconstructed code is treated as an EOA but for some reason
            // a constructor call to constructed code is also treated as EOA.
            match code_info_bytes[0] {
                1 => {
                    if is_constructed == is_constructor_call {
                        try_default_aa?
                    } else {
                        code_info_bytes
                    }
                }
                2 => {
                    if is_constructed == is_constructor_call {
                        try_default_aa?
                    } else {
                        is_evm = true;
                        evm_interpreter_code_hash
                    }
                }
                _ if code_info == U256::zero() => try_default_aa?,
                _ => return None,
            }
        };

        code_info[1] = 0;
        let code_key: U256 = U256::from_big_endian(&code_info);

        let cost = if self.decommitted_hashes.as_ref().contains_key(&code_key) {
            0
        } else {
            let code_length_in_words = u16::from_be_bytes([code_info[2], code_info[3]]);
            code_length_in_words as u32 * zkevm_opcode_defs::ERGS_PER_CODE_WORD_DECOMMITTMENT
        };

        Some((UnpaidDecommit { cost, code_key }, is_evm))
    }

    /// Returns the decommitted contract code and a flag set to `true` if this is a fresh decommit (i.e.,
    /// the code wasn't decommitted previously in the same VM run).
    #[doc(hidden)] // should be used for testing purposes only; can break VM operation otherwise
    pub fn decommit_opcode(&mut self, world: &mut dyn World, code_hash: U256) -> (Vec<u8>, bool) {
        let was_decommitted = self.decommitted_hashes.insert(code_hash, ()).is_some();
        (world.decommit_code(code_hash), !was_decommitted)
    }

    pub(crate) fn pay_for_decommit(
        &mut self,
        world: &mut dyn World,
        decommit: UnpaidDecommit,
        gas: &mut u32,
        statistics: &mut CircuitCycleStatistic,
    ) -> Option<Program> {
        if decommit.cost > *gas {
            // Unlike all other gas costs, this one is not paid if low on gas.
            return None;
        }
        *gas -= decommit.cost;
        let key = decommit.code_key;
        let old = self.decommitted_hashes.insert(key, ()).is_some();
        let decommit = world.decommit(decommit.code_key);

        // Each cycle of `CodeDecommitter` processes 2 words.
        // If the number of words in bytecode is odd, then number of cycles must be rounded up.
        if !old {
            let decommitter_cycles_used = (decommit.code_page().len() + 1) / 2;
            statistics.code_decommitter_cycles += decommitter_cycles_used as u32;
        }

        Some(decommit)
    }
}

pub(crate) struct UnpaidDecommit {
    cost: u32,
    code_key: U256,
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
