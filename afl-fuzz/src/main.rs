use differential_fuzzing::VmAndWorld;
use vm2::single_instruction_test::{add_heap_to_zk_evm, vm2_to_zk_evm, NoTracer, UniversalVmState};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(VmAndWorld { mut vm, mut world }) = arbitrary::Unstructured::new(data).arbitrary()
        {
            if vm.is_in_valid_state() && vm.instruction_is_not_precompile_call() {
                // Tests that running one instruction and converting to zk_evm produces the same result as
                // first converting to zk_evm and then running one instruction.

                let mut zk_evm = vm2_to_zk_evm(
                    &vm,
                    world.clone(),
                    vm.state.current_frame.pc_from_u16(0).unwrap(),
                );

                let pc = vm.run_single_instruction(&mut world).unwrap();
                assert!(vm.is_in_valid_state());

                add_heap_to_zk_evm(&mut zk_evm, &vm);
                let _ = zk_evm.cycle(&mut NoTracer);

                // vm2 does not build a frame for a failed far call, so we need to run the panic
                // to get a meaningful comparison.
                if vm.instruction_is_far_call() && zk_evm.local_state.pending_exception {
                    let _ = zk_evm.cycle(&mut NoTracer);
                }

                assert_eq!(
                    UniversalVmState::from(zk_evm),
                    vm2_to_zk_evm(&vm, world.clone(), pc).into()
                );
            }
        }
    });
}
