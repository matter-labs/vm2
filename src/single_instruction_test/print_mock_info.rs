use crate::{callframe::Callframe, State, VirtualMachine};

impl VirtualMachine {
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

impl State {
    pub fn print_mock_info(&self) {
        if let Some(heap) = self.heaps.read.read_that_happened() {
            println!("Heap: {:?}", heap);
        }
        if let Some((address, value)) = self.heaps.read.value_read.read.read_that_happened() {
            println!("  {value:?} read from {address:?}");
        }
        if let Some((address, value)) = self.heaps.read.value_read.write {
            println!("  {value:?} written to {address:?}");
        }

        println!("Current frame:");
        self.current_frame.print_mock_info();

        if let Some((pc, previous)) = self.previous_frames.first() {
            println!("Previous frame (pc at {pc}):");
            previous.print_mock_info();
        }
    }
}

impl Callframe {
    pub fn print_mock_info(&self) {
        if let Some((address, (value, tag))) = self.stack.read_that_happened() {
            println!("  {value:?} (is_pointer: {tag}) read from stack address {address}",);
        }
        if let Some((address, (value, tag))) = self.stack.write_that_happened() {
            println!("  {value:?} (is_pointer: {tag}) written to stack address {address}",);
        }
    }
}
