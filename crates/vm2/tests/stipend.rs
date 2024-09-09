#![cfg(not(feature = "single_instruction_test"))]

use primitive_types::U256;
use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2::{
    address_into_u256,
    addressing_modes::{
        Arguments, CodePage, Immediate1, Register, Register1, Register2, RegisterAndImmediate,
    },
    initial_decommit,
    testworld::TestWorld,
    ExecutionEnd, Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
};
use zksync_vm2_interface::opcodes::{self, Add};

const INITIAL_GAS: u32 = 1000;

fn test_scenario(gas_to_pass: u32) -> (ExecutionEnd, u32) {
    let r0 = Register::new(0);
    let r1 = Register::new(1);
    let r2 = Register::new(2);

    let ethereum_address = 0xeeeeee;
    let mut abi = U256::zero();
    abi.0[3] = gas_to_pass as u64;

    let main_program = Program::new(
        vec![
            Instruction::from_binop::<Add>(
                CodePage(RegisterAndImmediate {
                    immediate: 0,
                    register: r0,
                })
                .into(),
                Register2(r0),
                Register1(r1).into(),
                (),
                Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
                false,
                false,
            ),
            Instruction::from_binop::<Add>(
                CodePage(RegisterAndImmediate {
                    immediate: 1,
                    register: r0,
                })
                .into(),
                Register2(r0),
                Register1(r2).into(),
                (),
                Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
                false,
                false,
            ),
            Instruction::from_far_call::<opcodes::Normal>(
                Register1(r1),
                Register2(r2),
                // crash on error
                Immediate1(0xFFFF),
                false,
                false,
                Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
            ),
            Instruction::from_ret(
                Register1(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ),
        ],
        vec![abi, ethereum_address.into()],
    );

    let interpreter = Program::new(
        vec![
            Instruction::from_binop::<Add>(
                Register1(r0).into(),
                Register2(r0),
                Register1(r0).into(),
                (),
                Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
                false,
                false,
            ),
            Instruction::from_ret(
                Register1(Register::new(0)),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ),
        ],
        vec![],
    );

    let main_address = Address::from_low_u64_be(0xfeddeadbeef);
    let interpreter_address = Address::from_low_u64_be(0x1234567890abcdef);
    let mut world = TestWorld::new(&[
        (interpreter_address, interpreter),
        (main_address, main_program),
    ]);
    let interpreter_hash = world.address_to_hash[&address_into_u256(interpreter_address)].into();

    let mut ethereum_hash = [0; 32];
    ethereum_hash[0] = 2;
    world
        .address_to_hash
        .insert(ethereum_address.into(), ethereum_hash.into());

    let program = initial_decommit(&mut world, main_address);

    let mut vm = VirtualMachine::new(
        main_address,
        program,
        Address::zero(),
        vec![],
        INITIAL_GAS,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: interpreter_hash,
            hook_address: 0,
        },
    );

    let result = vm.run(&mut world, &mut ());
    (result, vm.state.current_frame.gas)
}

#[test]
fn test() {
    // without gas, relying on stipend
    let (result, gas_without_paying) = test_scenario(0);
    assert_eq!(result, ExecutionEnd::ProgramFinished(vec![]));
    assert!(gas_without_paying < INITIAL_GAS);

    // with gas
    let passed_gas = 500;
    let (result, gas_when_paying) = test_scenario(passed_gas);
    assert_eq!(result, ExecutionEnd::ProgramFinished(vec![]));
    assert!(gas_when_paying < INITIAL_GAS);

    assert!(
        gas_without_paying > gas_when_paying,
        "stipend should be used only when extra gas is needed"
    );

    // with insufficient gas
    let (result, gas_when_paying_one) = test_scenario(1);
    assert_eq!(result, ExecutionEnd::ProgramFinished(vec![]));
    assert!(gas_when_paying_one < INITIAL_GAS);

    assert_eq!(
        gas_without_paying,
        gas_when_paying_one + 1,
        "stipend should cover missing gas"
    );
}
