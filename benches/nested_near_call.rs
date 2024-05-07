use divan::{black_box, Bencher};
use u256::U256;
use vm2::{
    addressing_modes::{Immediate1, Immediate2, Register, Register1, Register2},
    initial_decommit,
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

    let storage_key_for_eth_balance = U256([
        4209092924407300373,
        6927221427678996148,
        4194905989268492595,
        15931007429432312239,
    ]);

    bencher.bench(|| {
        let mut world = Box::new(TestWorld::new(&[(address, program.clone())]));
        let program = initial_decommit(&mut *world, address);
        let mut vm = vm2::VirtualMachine::new(
            black_box(world),
            address,
            program,
            Address::zero(),
            vec![],
            80_000_000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
                storage_key_for_eth_balance: storage_key_for_eth_balance.into(),
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

    let storage_key_for_eth_balance = U256([
        4209092924407300373,
        6927221427678996148,
        4194905989268492595,
        15931007429432312239,
    ]);

    bencher.bench(|| {
        let mut world = Box::new(TestWorld::new(&[(address, program.clone())]));
        let program = initial_decommit(&mut *world, address);
        let mut vm = vm2::VirtualMachine::new(
            black_box(world),
            address,
            program,
            Address::zero(),
            vec![],
            80_000_000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
                storage_key_for_eth_balance: storage_key_for_eth_balance.into(),
            },
        );

        vm.run();
    });
}

fn main() {
    divan::main();
}
