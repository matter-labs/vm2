use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};
use zksync_vm2_interface::{CycleStats, Tracer};

use crate::{program::Program, world_diff::WorldDiff, World};

impl WorldDiff {
    pub(crate) fn decommit<T: Tracer>(
        &mut self,
        world: &mut impl World<T>,
        tracer: &mut T,
        address: U256,
        default_aa_code_hash: [u8; 32],
        evm_interpreter_code_hash: [u8; 32],
        is_constructor_call: bool,
    ) -> Option<(UnpaidDecommit, bool)> {
        let deployer_system_contract_address =
            Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW.into());

        let mut is_evm = false;

        let mut code_info = {
            let code_info = self.read_storage_without_refund(
                world,
                tracer,
                deployer_system_contract_address,
                address,
            );
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

        let was_decommitted = self.decommitted_hashes.as_ref().get(&code_key) == Some(&true);
        let cost = if was_decommitted {
            0
        } else {
            let code_length_in_words = u16::from_be_bytes([code_info[2], code_info[3]]);
            u32::from(code_length_in_words) * zkevm_opcode_defs::ERGS_PER_CODE_WORD_DECOMMITTMENT
        };

        Some((UnpaidDecommit { cost, code_key }, is_evm))
    }

    /// Returns the decommitted contract code and a flag set to `true` if this is a fresh decommit (i.e.,
    /// the code wasn't decommitted previously in the same VM run).
    #[doc(hidden)] // should be used for testing purposes only; can break VM operation otherwise
    pub fn decommit_opcode<T: Tracer>(
        &mut self,
        world: &mut impl World<T>,
        tracer: &mut T,
        code_hash: U256,
    ) -> (Vec<u8>, bool) {
        let is_new = self.decommitted_hashes.insert(code_hash, true) != Some(true);
        let code = world.decommit_code(code_hash);
        if is_new {
            let code_len = u32::try_from(code.len()).expect("bytecode length overflow");
            // Decommitter can process two words per cycle, hence division by 2 * 32 = 64.
            tracer.on_extra_prover_cycles(CycleStats::Decommit(code_len.div_ceil(64)));
        }
        (code, is_new)
    }

    pub(crate) fn pay_for_decommit<T: Tracer, W: World<T>>(
        &mut self,
        world: &mut W,
        tracer: &mut T,
        decommit: UnpaidDecommit,
        gas: &mut u32,
    ) -> Option<Program<T, W>> {
        if decommit.cost > *gas {
            // We intentionally record a decommitment event even if actual decommitment never happens because of an out-of-gas error.
            // This is how the old VM behaves.
            self.decommitted_hashes.insert(decommit.code_key, false);
            // Unlike all other gas costs, this one is not paid if low on gas.
            return None;
        }

        let is_new = self.decommitted_hashes.insert(decommit.code_key, true) != Some(true);
        *gas -= decommit.cost;

        let decommit = world.decommit(decommit.code_key);
        if is_new {
            let code_len_in_words =
                u32::try_from(decommit.code_page().len()).expect("bytecode length overflow");
            // Decommitter can process two words per cycle.
            tracer.on_extra_prover_cycles(CycleStats::Decommit(code_len_in_words.div_ceil(2)));
        }

        Some(decommit)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UnpaidDecommit {
    cost: u32,
    code_key: U256,
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
