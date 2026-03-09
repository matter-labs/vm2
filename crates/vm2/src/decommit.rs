use primitive_types::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};
use zksync_vm2_interface::{CycleStats, HeapId, Tracer};

use crate::{
    program::Program,
    world_diff::{DecommitState, WorldDiff},
    VirtualMachine, World,
};

/// Ensures that a decommit hash has a materialized reusable page and returns it.
///
/// The resulting page is pinned globally in [`WorldDiff`]. If that page is not owned by the
/// current frame, it is also recorded as kept alive in the bootloader frame (or current frame if
/// no bootloader frame exists), matching decommit opcode teardown semantics.
pub(crate) fn materialize_decommit_page<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    code_hash: U256,
    code: &[u8],
    candidate_page: HeapId,
) -> HeapId {
    if let Some(existing) = vm.world_diff.decommit_page(code_hash) {
        return existing;
    }

    let heap = vm.state.heaps.set_content_at(candidate_page, code);
    vm.world_diff.set_decommit_page(code_hash, heap);

    if heap != vm.state.current_frame.heap && heap != vm.state.current_frame.aux_heap {
        let heaps_to_keep_alive =
            if let Some(bootloader_frame) = vm.state.previous_frames.first_mut() {
                &mut bootloader_frame.heaps_i_am_keeping_alive
            } else {
                &mut vm.state.current_frame.heaps_i_am_keeping_alive
            };
        heaps_to_keep_alive.push(heap);
    }

    heap
}

impl WorldDiff {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decommit<T: Tracer>(
        &mut self,
        world: &mut impl World<T>,
        tracer: &mut T,
        address: U256,
        default_aa_code_hash: [u8; 32],
        evm_interpreter_code_hash: [u8; 32],
        is_constructor_call: bool,
        tx_number_in_block: u16,
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
                tx_number_in_block,
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

        let decommit_state = self.decommitted_hashes.as_ref().get(&code_key).copied();
        let was_decommitted = matches!(decommit_state, Some(DecommitState::Succeeded(_)));
        let cost = if was_decommitted {
            0
        } else {
            let code_length_in_words = u16::from_be_bytes([code_info[2], code_info[3]]);
            u32::from(code_length_in_words) * zkevm_opcode_defs::ERGS_PER_CODE_WORD_DECOMMITTMENT
        };

        let should_materialize = !was_decommitted;

        Some((
            UnpaidDecommit {
                cost,
                code_key,
                should_materialize,
            },
            is_evm,
        ))
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
        let is_new = match self.decommitted_hashes.as_ref().get(&code_hash).copied() {
            None | Some(DecommitState::Unsuccessful) => true,
            Some(DecommitState::Succeeded(_)) => false,
        };
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
            // We intentionally keep this hash visible even though decommit did not execute.
            // Legacy VM includes far-call OOG attempts in "used contracts", and shadow mode
            // compares that output.
            if !matches!(
                self.decommitted_hashes.as_ref().get(&decommit.code_key),
                Some(DecommitState::Succeeded(_))
            ) {
                self.decommitted_hashes
                    .insert(decommit.code_key, DecommitState::Unsuccessful);
            }
            // Unlike all other gas costs, this one is not paid if low on gas.
            return None;
        }

        let is_new = match self
            .decommitted_hashes
            .as_ref()
            .get(&decommit.code_key)
            .copied()
        {
            None | Some(DecommitState::Unsuccessful) => true,
            Some(DecommitState::Succeeded(_)) => false,
        };
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
    /// Indicates whether the decommit page should be materialized after successful payment.
    should_materialize: bool,
}

impl UnpaidDecommit {
    pub(crate) const fn code_key(self) -> U256 {
        self.code_key
    }

    pub(crate) const fn should_materialize(self) -> bool {
        self.should_materialize
    }
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
