use crate::{Instruction, World};
use std::sync::Arc;
use u256::{H160, U256};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

pub(crate) fn decommit(world: &mut dyn World, address: U256) -> (Arc<[Instruction]>, Arc<[U256]>) {
    let deployer_system_contract_address =
        Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

    let code_info = world.read_storage(deployer_system_contract_address, address);

    // TODO default address aliasing

    let mut code_info_bytes = [0; 32];
    code_info.to_big_endian(&mut code_info_bytes);

    if code_info_bytes[0] != 1 {
        panic!();
    }
    match code_info_bytes[1] {
        0 => {} // At rest
        1 => {} // constructed
        _ => {
            panic!();
        }
    }
    let code_length_in_words = u16::from_be_bytes([code_info_bytes[2], code_info_bytes[3]]);

    code_info_bytes[1] = 0;
    let code_key: U256 = U256::from_big_endian(&code_info_bytes);

    // TODO pay based on program length

    world.decommit(code_key)
}

pub(crate) fn address_into_u256(address: H160) -> U256 {
    let mut buffer = [0; 32];
    buffer[12..].copy_from_slice(address.as_bytes());
    U256::from_big_endian(&buffer)
}
