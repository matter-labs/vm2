use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2_interface::{CallframeInterface, StateInterface};

use crate::{
    testonly::{initial_decommit, TestWorld},
    ExecutionEnd, Program, Settings, VirtualMachine,
};

#[test]
fn call_to_invalid_address() {
    // A far call should make a new frame, even if the address is invalid.
    // Thus, setting the error handler to the call instruction itself should
    // result in an infinite loop.

    let address = Address::from_low_u64_be(0x1234567890abcdef);
    let bytecode = include_bytes!("bytecodes/call_far").to_vec();
    let mut world = TestWorld::new(&[(address, Program::new(bytecode, false))]);
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
