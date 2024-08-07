use differential_fuzzing::VmAndWorld;
use pretty_assertions::assert_eq;
use std::env;
use std::fs;
use vm2::single_instruction_test::add_heap_to_zk_evm;
use vm2::single_instruction_test::vm2_to_zk_evm;
use vm2::single_instruction_test::NoTracer;
use vm2::single_instruction_test::UniversalVmState;
use vm2::zkevm_opcode_defs::decoding::{EncodingModeProduction, VmEncodingMode};

fn main() {
    let filename = env::args()
        .nth(1)
        .expect("Please provide the test case to show as argument.");

    let bytes = fs::read(filename).expect("Failed to read file");

    let VmAndWorld { mut vm, mut world } =
        arbitrary::Unstructured::new(&bytes).arbitrary().unwrap();

    println!("{:?}", vm.state);
    assert!(vm.is_in_valid_state());

    let mut zk_evm = vm2_to_zk_evm(
        &vm,
        world.clone(),
        vm.state.current_frame.pc_from_u16(0).unwrap(),
    );

    let (parsed, _) = EncodingModeProduction::parse_preliminary_variant_and_absolute_number(
        vm.state.current_frame.raw_first_instruction(),
    );
    println!("{}", parsed);
    let pc = vm.run_single_instruction(&mut world).unwrap();

    println!("Mocks that have been touched:");
    vm.print_mock_info();

    assert!(vm.is_in_valid_state());

    add_heap_to_zk_evm(&mut zk_evm, &vm);
    let _ = zk_evm.cycle(&mut NoTracer);

    // vm2 does not build a frame for a failed far call, so we need to run the panic
    // to get a meaningful comparison.
    if vm.instruction_is_far_call() && zk_evm.local_state.pending_exception {
        let _ = zk_evm.cycle(&mut NoTracer);
    }

    assert_eq!(
        UniversalVmState::from(zk_evm),
        vm2_to_zk_evm(&vm, world.clone(), pc).into()
    );
}
