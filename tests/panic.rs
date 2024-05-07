use proptest::prelude::*;
use u256::U256;
use vm2::{
    addressing_modes::{Immediate1, Immediate2, Register, Register1},
    initial_decommit,
    testworld::TestWorld,
    ExecutionEnd, Instruction, Predicate, Program, VirtualMachine,
};
use zkevm_opcode_defs::ethereum_types::Address;

proptest! {
    #[test]
    fn panic_to_varying_label(label: u16) {
        let mut instructions = vec![
            Instruction::from_near_call(
                Register1(Register::new(0)),
                Immediate1(1),
                Immediate2(0xFFFF),
                Predicate::Always,
            ),
            Instruction::from_panic(Some(Immediate1(label)), Predicate::Always),
        ];
        for _ in 0..98 {
            instructions.push(Instruction::from_ret(
                Register1(Register::new(0)),
                None,
                vm2::Predicate::Always,
            ));
        }

        let program = Program::new(instructions, vec![]);

        let address = Address::from_low_u64_be(0x1234567890abcdef);
        let mut world = TestWorld::new(&[(address, program)]);
        let program = initial_decommit(&mut world, address);
        let storage_key_for_eth_balance = U256([
            4209092924407300373,
            6927221427678996148,
            4194905989268492595,
            15931007429432312239,
        ]);

        let mut vm = VirtualMachine::new(
            Box::new(world),
            address,
            program,
            Address::zero(),
            vec![],
            1000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
                storage_key_for_eth_balance: storage_key_for_eth_balance.into(),
            },
        );

        assert_eq!(vm.run(),
            if 1 < label && label < 100 {
                ExecutionEnd::ProgramFinished(vec![])
            } else {
                ExecutionEnd::Panicked
            });
    }
}
