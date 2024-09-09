use std::{env, fs};

use pretty_assertions::assert_eq;
use zkevm_opcode_defs::decoding::{EncodingModeProduction, VmEncodingMode};
use zksync_vm2::single_instruction_test::{
    add_heap_to_zk_evm, vm2_to_zk_evm, NoTracer, UniversalVmState,
};
use zksync_vm2_afl_fuzz::VmAndWorld;

fn main() {
    let filename = env::args()
        .nth(1)
        .expect("Please provide the test case to show as argument.");

    let bytes = fs::read(filename).expect("Failed to read file");

    let VmAndWorld { mut vm, mut world } =
        arbitrary::Unstructured::new(&bytes).arbitrary().unwrap();

    println!("{:?}", vm.state);
    assert!(vm.is_in_valid_state());

    let mut zk_evm = vm2_to_zk_evm(&vm, world.clone());

    let (parsed, _) = EncodingModeProduction::parse_preliminary_variant_and_absolute_number(
        vm.state.current_frame.raw_first_instruction(),
    );
    println!("{}", parsed);
    vm.run_single_instruction(&mut world, &mut ());

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
        vm2_to_zk_evm(&vm, world.clone()).into()
    );
}
