use divan::{black_box, Bencher};
use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2::{
    addressing_modes::{Arguments, Immediate1, Immediate2, Register, Register1, Register2},
    initial_decommit,
    testworld::TestWorld,
    Instruction, ModeRequirements,
    Predicate::Always,
    Program, Settings, VirtualMachine,
};

#[divan::bench]
fn nested_near_call(bencher: Bencher) {
    let program = Program::from_raw(
        vec![Instruction::from_near_call(
            // zero means pass all gas
            Register1(Register::new(0)),
            Immediate1(0),
            Immediate2(0),
            Arguments::new(Always, 10, ModeRequirements::none()),
        )],
        vec![],
    );

    let address = Address::from_low_u64_be(0xabe123ff);

    bencher.bench(|| {
        let mut world = TestWorld::new(&[(address, program.clone())]);
        let program = initial_decommit(&mut world, address);
        let mut vm = VirtualMachine::new(
            address,
            program,
            Address::zero(),
            vec![],
            10_000_000,
            Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        vm.run(black_box(&mut world), &mut ());
    });
}

#[divan::bench]
fn nested_near_call_with_storage_write(bencher: Bencher) {
    let program = Program::from_raw(
        vec![
            Instruction::from_ergs_left(
                Register1(Register::new(1)),
                Arguments::new(Always, 5, ModeRequirements::none()),
            ),
            Instruction::from_sstore(
                // always use same storage slot to get a warm write discount
                Register1(Register::new(0)),
                Register2(Register::new(1)),
                Arguments::new(Always, 5511, ModeRequirements::none()), // need to use actual cost to not create free gas from refunds
            ),
            Instruction::from_near_call(
                // zero means pass all gas
                Register1(Register::new(0)),
                Immediate1(0),
                Immediate2(0),
                Arguments::new(Always, 25, ModeRequirements::none()),
            ),
        ],
        vec![],
    );

    let address = Address::from_low_u64_be(0xabe123ff);

    bencher.bench(|| {
        let mut world = TestWorld::new(&[(address, program.clone())]);
        let program = initial_decommit(&mut world, address);
        let mut vm = VirtualMachine::new(
            address,
            program,
            Address::zero(),
            vec![],
            80_000_000,
            Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        vm.run(black_box(&mut world), &mut ());
    });
}

fn main() {
    divan::main();
}
