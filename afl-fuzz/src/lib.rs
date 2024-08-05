use arbitrary::Arbitrary;
use vm2::{single_instruction_test::MockWorld, VirtualMachine};

#[derive(Arbitrary, Debug)]
pub struct VmAndWorld<T> {
    pub vm: VirtualMachine<T>,
    pub world: MockWorld,
}
