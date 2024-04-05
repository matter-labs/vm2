use std::sync::Arc;

use divan::{black_box, Bencher};
use vm2::{
    addressing_modes::{Immediate1, Immediate2, Register, Register1, Register2},
    Instruction,
    Predicate::Always,
    World,
};
use zkevm_opcode_defs::ethereum_types::Address;

#[divan::bench]
fn nested_near_call(bencher: Bencher) {
    let program = Arc::new([Instruction::from_near_call(
        // zero means pass all gas
        Register1(Register::new(0)),
        Immediate1(0),
        Immediate2(0),
        Always,
    )]);

    bencher.bench(|| {
        let mut vm = vm2::VirtualMachine::new(
            Box::new(TestWorld(black_box(program.clone()))),
            Address::zero(),
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
    let program = Arc::new([
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
    ]);

    bencher.bench(|| {
        let mut vm = vm2::VirtualMachine::new(
            Box::new(TestWorld(black_box(program.clone()))),
            Address::zero(),
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

struct TestWorld(Arc<[Instruction]>);
impl World for TestWorld {
    fn decommit(&mut self, _: u256::U256) -> (Arc<[Instruction]>, Arc<[u256::U256]>) {
        (self.0.clone(), Arc::new([]))
    }

    fn read_storage(&mut self, _: u256::H160, _: u256::U256) -> u256::U256 {
        0.into()
    }

    fn handle_hook(&mut self, _: u32, _: &mut vm2::State) {
        unreachable!()
    }
}

fn main() {
    divan::main();
}
