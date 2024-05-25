use crate::{callframe::Callframe, State, VirtualMachine};

impl VirtualMachine {
    pub fn print_mock_info(&self) {
        // TODO world_diff
        self.state.print_mock_info();
    }
}

impl State {
    pub fn print_mock_info(&self) {
        self.current_frame.print_mock_info();
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
            println!("{value:?} (is_pointer: {tag}) read from stack address {address}",);
        }
        if let Some((address, (value, tag))) = self.stack.write_that_happened() {
            println!("{value:?} (is_pointer: {tag}) written to stack address {address}",);
        }
    }
}
