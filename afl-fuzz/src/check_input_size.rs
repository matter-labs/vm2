//! Finds out how many bytes of data have to be provided to build the mock state.

use differential_fuzzing::VmAndWorld;

fn main() {
    let data = [2; 10000];
    let mut u = arbitrary::Unstructured::new(&data);
    let _: VmAndWorld = u.arbitrary().unwrap();
    println!("{:?}", u.len());
}
