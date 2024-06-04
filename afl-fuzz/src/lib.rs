use arbitrary::Arbitrary;
use vm2::{single_instruction_test::MockWorld, VirtualMachine};

#[derive(Arbitrary, Debug)]
pub struct VmAndWorld {
    pub vm: VirtualMachine,
    pub world: MockWorld,
}
