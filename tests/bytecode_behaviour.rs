#![cfg(not(feature = "single_instruction_test"))]

use eravm_stable_interface::Tracer;
use u256::U256;
use vm2::{
    decode::decode_program, initial_decommit, testworld::TestWorld, ExecutionEnd, Program,
    VirtualMachine, World,
};
use zkevm_opcode_defs::ethereum_types::Address;

fn program_from_file<T: Tracer, W: World<T>>(filename: &str) -> Program<T, W> {
    let blob = std::fs::read(filename).unwrap();
    Program::new(
        decode_program(
            &blob
                .chunks_exact(8)
                .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
                .collect::<Vec<_>>(),
            false,
        ),
        blob.chunks_exact(32)
            .map(U256::from_big_endian)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn call_to_invalid_address() {
    // A far call should make a new frame, even if the address is invalid.
    // Thus, setting the error handler to the call instruction itself should
    // result in an infinite loop.

    let address = Address::from_low_u64_be(0x1234567890abcdef);
    let mut world = TestWorld::new(&[(address, program_from_file("tests/bytecodes/call_far"))]);
    let program = initial_decommit(&mut world, address);

    let mut vm = VirtualMachine::new(
        address,
        program,
        Address::zero(),
        vec![],
        10000,
        vm2::Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );
    assert!(matches!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::Panicked
    ));
    assert_eq!(vm.state.current_frame.gas, 0);
}
