use arbitrary::Arbitrary;
use eravm_stable_interface::Tracer;
use vm2::{single_instruction_test::MockWorld, VirtualMachine};

#[derive(Arbitrary, Debug)]
pub struct VmAndWorld<T: Tracer> {
    pub vm: VirtualMachine<T>,
    pub world: MockWorld,
}
