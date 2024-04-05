#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use u256::H160;
use vm2::{
    jump_to_beginning, testworld::TestWorld, Instruction, Program, Settings, VirtualMachine,
};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let mut program: Vec<Instruction> = Arbitrary::arbitrary(&mut u).unwrap();

    if program.len() >= 1 << 16 {
        program.truncate(1 << 16);
        program.push(jump_to_beginning());
    } else {
        program.push(Instruction::from_invalid());
    }

    let address = H160::from_low_u64_be(0x1234567890abcdef);
    let world = TestWorld::new(&[(address, Program::new(program, vec![]))]);

    let mut state = VirtualMachine::new(
        Box::new(world),
        address,
        H160::zero(),
        vec![],
        u32::MAX,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );
    state.run();
});
