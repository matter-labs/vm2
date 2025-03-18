use zksync_vm2::single_instruction_test::{
    add_heap_to_zk_evm, vm2_to_zk_evm, NoTracer, UniversalVmState,
};
use zksync_vm2_afl_fuzz::VmAndWorld;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(VmAndWorld { mut vm, mut world }) = arbitrary::Unstructured::new(data).arbitrary()
        {
            if vm.is_in_valid_state() && vm.instruction_is_not_precompile_call() {
                let is_far_call = vm.instruction_is_far_call();

                // Tests that running one instruction and converting to zk_evm produces the same result as
                // first converting to zk_evm and then running one instruction.

                let mut zk_evm = vm2_to_zk_evm(&vm, world.clone());

                vm.run_single_instruction(&mut world, &mut ());
                assert!(vm.is_in_valid_state());

                add_heap_to_zk_evm(&mut zk_evm, &vm);
                let _ = zk_evm.cycle(&mut NoTracer);

                // zk_evm's far call sometimes passes calldata or declares the new frame and EVM frame
                // even though the frame is immediately popped because the far call failed.
                // We don't need to replicate all that, so we compare the state after the panic.
                if is_far_call && zk_evm.local_state.pending_exception {
                    vm.run_single_instruction(&mut world, &mut ());
                    let _ = zk_evm.cycle(&mut NoTracer);
                }

                assert_eq!(
                    UniversalVmState::from(zk_evm),
                    vm2_to_zk_evm(&vm, world.clone()).into()
                );
            }
        }
    });
}
