use zksync_vm2_interface::Tracer;

use crate::{callframe::Callframe, state::State, VirtualMachine};

impl<T: Tracer, W> VirtualMachine<T, W> {
    pub fn print_mock_info(&self) {
        self.state.print_mock_info();
        println!("Events: {:?}", self.world_diff.events());
        println!("Logs: {:?}", self.world_diff.l2_to_l1_logs());
        println!(
            "Storage changes: {:?}",
            self.world_diff.get_storage_changes().collect::<Vec<_>>()
        );
    }
}

impl<T: Tracer, W> State<T, W> {
    pub(crate) fn print_mock_info(&self) {
        if let Some((heap_id, heap)) = self.heaps.read.read_that_happened() {
            println!("Heap: {heap_id:?}");
            if let Some((address, value)) = heap.read.read_that_happened() {
                println!("  {value:?} read from {address:?}");
            }
            if let Some((address, value)) = heap.write {
                println!("  {value:?} written to {address:?}");
            }
        }

        println!("Current frame:");
        self.current_frame.print_mock_info();

        if let Some(previous) = self.previous_frames.first() {
            println!("Previous frame (pc at {}):", previous.get_pc_as_u16());
            previous.print_mock_info();
        }
    }
}

impl<T, W> Callframe<T, W> {
    pub(crate) fn print_mock_info(&self) {
        if let Some((address, (value, tag))) = self.stack.read_that_happened() {
            println!("  {value:?} (is_pointer: {tag}) read from stack address {address}",);
        }
        if let Some((address, (value, tag))) = self.stack.write_that_happened() {
            println!("  {value:?} (is_pointer: {tag}) written to stack address {address}",);
        }
    }
}
