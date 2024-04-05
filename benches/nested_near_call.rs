use divan::{black_box, Bencher};
use vm2::{
    addressing_modes::{Immediate1, Immediate2, Register, Register1, Register2},
    testworld::TestWorld,
    Instruction,
    Predicate::Always,
    Program,
};
use zkevm_opcode_defs::ethereum_types::Address;

#[divan::bench]
fn nested_near_call(bencher: Bencher) {
    let program = Program::new(
        vec![Instruction::from_near_call(
            // zero means pass all gas
            Register1(Register::new(0)),
            Immediate1(0),
            Immediate2(0),
            Always,
        )],
        vec![],
    );

    let address = Address::from_low_u64_be(0xabe123ff);

    bencher.bench(|| {
        let world = Box::new(TestWorld::new(&[(address, program.clone())]));
        let mut vm = vm2::VirtualMachine::new(
            black_box(world),
            address,
            Address::zero(),
            vec![],
            80_000_000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        vm.run();
    });
}

#[divan::bench]
fn nested_near_call_with_storage_write(bencher: Bencher) {
    let program = Program::new(
        vec![
            Instruction::from_ergs_left(Register1(Register::new(1)), Always),
            Instruction::from_sstore(
                // always use same storage slot to get a warm write discount
                Register1(Register::new(0)),
                Register2(Register::new(1)),
                Always,
            ),
            Instruction::from_near_call(
                // zero means pass all gas
                Register1(Register::new(0)),
                Immediate1(0),
                Immediate2(0),
                Always,
            ),
        ],
        vec![],
    );

    let address = Address::from_low_u64_be(0xabe123ff);

    bencher.bench(|| {
        let world = Box::new(TestWorld::new(&[(address, program.clone())]));
        let mut vm = vm2::VirtualMachine::new(
            black_box(world),
            address,
            Address::zero(),
            vec![],
            80_000_000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        vm.run();
    });
}

fn main() {
    divan::main();
}
