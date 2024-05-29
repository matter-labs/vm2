use arbitrary::Arbitrary;
use vm2::{MockWorld, VirtualMachine};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(VmAndWorld { mut vm, mut world }) = arbitrary::Unstructured::new(data).arbitrary()
        {
            if vm.is_in_valid_state() && vm.instruction_is_not_precompile_call() {
                let _ = vm.run_single_instruction(&mut world);
                assert!(vm.is_in_valid_state());
            }
        }
    });
}

#[derive(Arbitrary, Debug)]
struct VmAndWorld {
    vm: VirtualMachine,
    world: MockWorld,
}
