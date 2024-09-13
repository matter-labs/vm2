#![cfg(not(feature = "single_instruction_test"))]

use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2::{
    initial_decommit, testworld::TestWorld, ExecutionEnd, Program, Settings, VirtualMachine, World,
};
use zksync_vm2_interface::{CallframeInterface, StateInterface, Tracer};

fn program_from_file<T: Tracer, W: World<T>>(filename: &str) -> Program<T, W> {
    let blob = std::fs::read(filename).unwrap();
    Program::new(blob, false)
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
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );
    assert!(matches!(
        vm.run(&mut world, &mut ()),
        ExecutionEnd::Panicked
    ));
    assert_eq!(vm.current_frame().gas(), 0);
}
