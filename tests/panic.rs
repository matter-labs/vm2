use proptest::prelude::*;
use vm2::{
    addressing_modes::{Immediate1, Immediate2, Register, Register1},
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
        let world = TestWorld::new(&[(address, program)]);

        let mut vm = VirtualMachine::new(
            Box::new(world),
            address,
            Address::zero(),
            vec![],
            1000,
            vm2::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
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
