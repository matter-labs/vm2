use arbitrary::Arbitrary;
use vm2::{MockWorld, VirtualMachine};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(VmAndWorld { mut vm, mut world }) = arbitrary::Unstructured::new(data).arbitrary()
        {
            let instruction = vm.get_first_instruction();
            let result = vm.run_single_instruction(instruction, &mut world);
        }
    });
}

#[derive(Arbitrary, Debug)]
struct VmAndWorld {
    vm: VirtualMachine,
    world: MockWorld,
}
