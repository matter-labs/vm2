use std::collections::HashSet;

use primitive_types::{H160, U256};
use zkevm_opcode_defs::ethereum_types::Address;
use zksync_vm2_interface::{opcodes, CallframeInterface, StateInterface};

use crate::{
    addressing_modes::{
        Arguments, CodePage, Immediate1, Register, Register1, Register2, RegisterAndImmediate,
    },
    testonly::{initial_decommit, TestWorld},
    ExecutionEnd, Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
};

const GAS_TO_PASS: u32 = 10_000;
const LARGE_BYTECODE_LEN: usize = 10_000;
const MAIN_ADDRESS: Address = Address::repeat_byte(0x23);
const CALLED_ADDRESS: Address = H160([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xee, 0xee, 0xee, 0xee,
]);

fn create_test_world() -> TestWorld<()> {
    let r0 = Register::new(0);
    let r1 = Register::new(1);
    let r2 = Register::new(2);

    let mut abi = U256::zero();
    abi.0[3] = GAS_TO_PASS.into();

    let main_program = Program::from_raw(
        vec![
            // 0..=2: Prepare and execute far call
            Instruction::from_add(
                CodePage(RegisterAndImmediate {
                    immediate: 0,
                    register: r0,
                })
                .into(),
                Register2(r0),
                Register1(r1).into(),
                Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
                false,
                false,
            ),
            Instruction::from_add(
                CodePage(RegisterAndImmediate {
                    immediate: 1,
                    register: r0,
                })
                .into(),
                Register2(r0),
                Register1(r2).into(),
                Arguments::new(Predicate::Always, 6, ModeRequirements::none()),
                false,
                false,
            ),
            Instruction::from_far_call::<opcodes::Normal>(
                Register1(r1),
                Register2(r2),
                Immediate1(5), // revert exception handler
                false,
                false,
                Arguments::new(Predicate::Always, 200, ModeRequirements::none()),
            ),
            // 3: Hook (0)
            Instruction::from_heap_write(
                Register1(r0).into(),
                Register2(r0),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
                true,
            ),
            // 4: Jump to program start
            Instruction::from_jump(
                Immediate1(0).into(),
                Register1(r0),
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ),
            // 5: Revert exception handler
            Instruction::from_revert(
                Register1(r0),
                None,
                Arguments::new(Predicate::Always, 5, ModeRequirements::none()),
            ),
        ],
        vec![abi, CALLED_ADDRESS.to_low_u64_be().into()],
    );

    let called_program = Program::from_raw(
        vec![
            Instruction::from_add(
                Register1(r0).into(),
                Register2(r0),
                Register1(r0).into(),
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
        vec![U256::zero(); LARGE_BYTECODE_LEN],
    );

    TestWorld::new(&[
        (CALLED_ADDRESS, called_program),
        (MAIN_ADDRESS, main_program),
    ])
}

#[test]
fn test() {
    let mut world = create_test_world();
    let main_program = initial_decommit(&mut world, MAIN_ADDRESS);
    let initial_gas = 1_000_000;
    let mut vm = VirtualMachine::new(
        MAIN_ADDRESS,
        main_program,
        Address::zero(),
        &[],
        initial_gas,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );

    let result = vm.run(&mut world, &mut ());
    let remaining_gas = vm.current_frame().gas();
    assert_eq!(result, ExecutionEnd::SuspendedOnHook(0));
    let expected_decommit_cost = u32::try_from(LARGE_BYTECODE_LEN).unwrap() * 4;
    assert!(
        remaining_gas < initial_gas - expected_decommit_cost,
        "{remaining_gas}"
    );

    // Check that the decommitment is not charged when the decommitment happens the second time.
    vm.run(&mut world, &mut ());
    let new_remaining_gas = vm.current_frame().gas();
    assert_eq!(result, ExecutionEnd::SuspendedOnHook(0));
    assert!(
        remaining_gas - new_remaining_gas < expected_decommit_cost,
        "{remaining_gas}, {new_remaining_gas}"
    );
}

#[test]
fn test_with_initial_out_of_gas_error() {
    let mut world = create_test_world();
    let main_program = initial_decommit(&mut world, MAIN_ADDRESS);
    let mut vm = VirtualMachine::new(
        MAIN_ADDRESS,
        main_program,
        Address::zero(),
        &[],
        10_000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );

    let result = vm.run(&mut world, &mut ());
    assert_eq!(result, ExecutionEnd::Reverted(vec![]));
    // Unsuccessful decommit should still be returned in `decommitted_hashes()`
    let decommitted: HashSet<_> = vm.world_diff().decommitted_hashes().collect();
    let called_bytecode_hash = world.address_to_hash[&CALLED_ADDRESS.to_low_u64_be().into()];
    assert!(
        decommitted.contains(&called_bytecode_hash),
        "{decommitted:?}"
    );

    // Recover the VM and increase the amount of gas passed to the far call.
    vm.current_frame().set_program_counter(0);
    vm.current_frame().set_gas(1_000_000);

    let initial_gas = vm.current_frame().gas();
    let result = vm.run(&mut world, &mut ());
    let remaining_gas = vm.current_frame().gas();
    assert_eq!(result, ExecutionEnd::SuspendedOnHook(0));
    let expected_decommit_cost = u32::try_from(LARGE_BYTECODE_LEN).unwrap() * 4;
    assert!(
        remaining_gas < initial_gas - expected_decommit_cost,
        "{remaining_gas}"
    );
}
