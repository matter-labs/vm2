use std::sync::Arc;
use u256::U256;
use vm2::{address_into_u256, decode::decode_program, ExecutionEnd, Instruction, State, World};
use zkevm_opcode_defs::{
    ethereum_types::Address, system_params::DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW,
};

fn program_from_file(filename: &str) -> (Arc<[Instruction]>, Arc<[U256]>) {
    let blob = std::fs::read(filename).unwrap();
    (
        decode_program(
            &blob
                .chunks_exact(8)
                .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
                .collect::<Vec<_>>(),
        )
        .into(),
        blob.chunks_exact(32)
            .map(|chunk| U256::from_big_endian(chunk.try_into().unwrap()))
            .collect::<Vec<_>>()
            .into(),
    )
}

#[test]
fn call_to_invalid_address() {
    // A far call should make a new frame, even if the address is invalid.
    // Thus, setting the error handler to the call instruction itself should
    // result in an infinite loop.

    struct TestWorld;
    impl World for TestWorld {
        fn decommit(&mut self, hash: u256::U256) -> (Arc<[Instruction]>, Arc<[u256::U256]>) {
            let code_hash = {
                let mut abi = [0u8; 32];
                abi[0] = 1;
                U256::from_big_endian(&abi)
            };

            if hash == code_hash {
                program_from_file("tests/bytecodes/call_far")
            } else {
                panic!("unexpected decommit")
            }
        }

        fn read_storage(&mut self, contract: u256::H160, key: u256::U256) -> u256::U256 {
            let deployer_system_contract_address =
                Address::from_low_u64_be(DEPLOYER_SYSTEM_CONTRACT_ADDRESS_LOW as u64);

            let code_hash = {
                let mut abi = [0u8; 32];
                abi[0] = 1;
                U256::from_big_endian(&abi)
            };

            if contract == deployer_system_contract_address
                && key == address_into_u256(Address::from_low_u64_be(0x1234567890abcdef))
            {
                code_hash
            } else {
                0.into()
            }
        }
    }

    let mut vm = State::new(
        Box::new(TestWorld),
        Address::from_low_u64_be(0x1234567890abcdef),
        Address::zero(),
        vec![],
        1000,
    );
    assert!(matches!(vm.run(), ExecutionEnd::Panicked(_)));
    assert_eq!(vm.current_frame.gas, 0);
}
