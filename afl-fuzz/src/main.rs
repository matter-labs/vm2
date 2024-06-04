use differential_fuzzing::VmAndWorld;
use vm2::single_instruction_test::{vm2_to_zk_evm, NoTracer, UniversalVmState};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(VmAndWorld { mut vm, mut world }) = arbitrary::Unstructured::new(data).arbitrary()
        {
            if vm.is_in_valid_state() && vm.instruction_is_not_precompile_call() {
                // Tests that running one instruction and converting to zk_evm produces the same result as
                // first converting to zk_evm and then running one instruction.

                let mut zk_evm = vm2_to_zk_evm(&vm, world.clone());

                let _ = vm.run_single_instruction(&mut world);
                assert!(vm.is_in_valid_state());

                let _ = zk_evm.cycle(&mut NoTracer);
                assert_eq!(
                    UniversalVmState::from(zk_evm),
                    vm2_to_zk_evm(&vm, world.clone()).into()
                );
                // TODO compare emitted events, storage changes and pubdata
            }
        }
    });
}
