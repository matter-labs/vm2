use arbitrary::Arbitrary;
use zksync_vm2::{single_instruction_test::MockWorld, VirtualMachine};
use zksync_vm2_interface::Tracer;

#[derive(Arbitrary, Debug)]
pub struct VmAndWorld<T: Tracer> {
    pub vm: VirtualMachine<T, MockWorld>,
    pub world: MockWorld,
}
