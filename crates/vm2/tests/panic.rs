#![cfg(not(feature = "single_instruction_test"))]

use proptest::prelude::*;
use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2::{
    addressing_modes::{Arguments, Immediate1, Immediate2, Register, Register1},
    initial_decommit,
    testworld::TestWorld,
    ExecutionEnd, Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
};

proptest! {
    #[test]
    fn panic_to_varying_label(label: u16) {
        let mut instructions = vec![
            Instruction::from_near_call(
                Register1(Register::new(0)),
                Immediate1(1),
                Immediate2(0xFFFF),
                Arguments::new(Predicate::Always, 25, ModeRequirements::none()),
            ),
            Instruction::from_panic(Some(Immediate1(label)), Arguments::new(Predicate::Always, 5, ModeRequirements::none())),
        ];
        for _ in 0..98 {
            instructions.push(Instruction::from_ret(
                Register1(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ));
        }

        let program = Program::new(instructions, vec![]);

        let address = Address::from_low_u64_be(0x1234567890abcdef);
        let mut world = TestWorld::new(&[(address, program)]);
        let program = initial_decommit(&mut world, address);

        let mut vm = VirtualMachine::new(
            address,
            program,
            Address::zero(),
            vec![],
            1000,
            Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        assert_eq!(vm.run(&mut world, &mut ()),
            if 1 < label && label < 100 {
                ExecutionEnd::ProgramFinished(vec![])
            } else {
                ExecutionEnd::Panicked
            });
    }
}
