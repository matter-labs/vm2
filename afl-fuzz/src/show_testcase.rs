use arbitrary::Arbitrary;
use std::env;
use std::fs;
use vm2::zkevm_opcode_defs::decoding::{EncodingModeProduction, VmEncodingMode};
use vm2::MockWorld;
use vm2::VirtualMachine;

fn main() {
    let filename = env::args()
        .nth(1)
        .expect("Please provide the test case to show as argument.");

    let bytes = fs::read(filename).expect("Failed to read file");

    let VmAndWorld { mut vm, mut world } =
        arbitrary::Unstructured::new(&bytes).arbitrary().unwrap();

    println!("{:?}", vm.state);
    assert!(vm.is_in_valid_state());

    let (parsed, _) = EncodingModeProduction::parse_preliminary_variant_and_absolute_number(
        vm.state.current_frame.raw_first_instruction(),
    );
    println!("{}", parsed);
    let _ = vm.run_single_instruction(&mut world);

    println!("{:?}", vm.state);
    assert!(vm.is_in_valid_state());
}

#[derive(Arbitrary, Debug)]
struct VmAndWorld {
    vm: VirtualMachine,
    world: MockWorld,
}
