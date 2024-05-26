//! Finds out how many bytes of data have to be provided to build the mock state.

use arbitrary::Arbitrary;
use vm2::{MockWorld, VirtualMachine};

fn main() {
    let data = [2; 10000];
    let mut u = arbitrary::Unstructured::new(&data);
    let _: VmAndWorld = u.arbitrary().unwrap();
    println!("{:?}", u.len());
}

#[derive(Arbitrary, Debug)]
struct VmAndWorld {
    _vm: VirtualMachine,
    _world: MockWorld,
}
