use crate::{instruction::Panic, Instruction, World};
use std::sync::Arc;
use u256::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

pub(crate) struct CodeInfo {
    pub is_constructed: bool,
    pub code_length_in_words: u16,
}

pub(crate) fn decommit(
    world: &mut dyn World,
    address: U256,
    default_aa_code_hash: U256,
) -> Result<(Arc<[Instruction]>, Arc<[U256]>, CodeInfo), Panic> {
    let deployer_system_contract_address =
        Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

    let code_info = {
        let code_info = world.read_storage(deployer_system_contract_address, address);
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
    let code_length_in_words = u16::from_be_bytes([code_info_bytes[2], code_info_bytes[3]]);

    code_info_bytes[1] = 0;
    let code_key: U256 = U256::from_big_endian(&code_info_bytes);

    // TODO pay based on program length

    let (program, code_page) = world.decommit(code_key);
    Ok((
        program,
        code_page,
        CodeInfo {
            is_constructed,
            code_length_in_words,
        },
    ))
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
