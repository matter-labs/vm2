use std::env;
use std::fs;

use arbitrary::Arbitrary;
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

    let instruction = vm.get_first_instruction();
    vm.run_single_instruction(instruction, &mut world);
}

#[derive(Arbitrary, Debug)]
struct VmAndWorld {
    vm: VirtualMachine,
    world: MockWorld,
}
