//! Finds out how many bytes of data have to be provided to build the mock state.

use differential_fuzzing::VmAndWorld;

fn main() {
    const BYTES_GIVEN: usize = 10000;
    let data = [0xFF; BYTES_GIVEN];
    let mut u = arbitrary::Unstructured::new(&data);
    let _: VmAndWorld = u.arbitrary().unwrap();
    println!("{:?}", BYTES_GIVEN - u.len());
}
