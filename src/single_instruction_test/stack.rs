use super::{
    mock_array::MockRead, validation::is_valid_tagged_value, vm::arbitrary_register_value,
};
use crate::HeapId;
use u256::U256;

#[derive(PartialEq, Debug, Clone)]
pub struct Stack {
    read: MockRead<u16, (U256, bool)>,
    slot_written: Option<u16>,
    value_written: U256,
    pointer_tag_written: bool,
}

impl Stack {
    pub(crate) fn new_arbitrary(
        u: &mut arbitrary::Unstructured,
        calldata_heap: HeapId,
        base_page: u32,
    ) -> arbitrary::Result<Self> {
        Ok(Self {
            read: MockRead::new(arbitrary_register_value(u, calldata_heap, base_page)?),
            slot_written: None,
            value_written: 0.into(),
            pointer_tag_written: false,
        })
    }

    pub(crate) fn get(&self, slot: u16) -> U256 {
        assert!(self.slot_written.is_none());
        self.read.get(slot).0
    }

    pub(crate) fn get_with_pointer_flag(&self, slot: u16) -> (U256, bool) {
        assert!(self.slot_written.is_none());
        *self.read.get(slot)
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256, is_pointer: bool) {
        self.assert_write_to_same_slot(slot);
        self.value_written = value;
        self.pointer_tag_written = is_pointer;
    }

    fn assert_write_to_same_slot(&mut self, slot: u16) {
        if let Some(last_slot) = self.slot_written {
            assert!(last_slot == slot);
        }
        self.slot_written = Some(slot);
    }

    pub fn read_that_happened(&self) -> Option<(u16, (U256, bool))> {
        self.read.read_that_happened()
    }

    pub fn write_that_happened(&self) -> Option<(u16, (U256, bool))> {
        self.slot_written
            .map(|slot| (slot, (self.value_written, self.pointer_tag_written)))
    }

    pub fn is_valid(&self) -> bool {
        is_valid_tagged_value(self.read.value_read)
            && (self.slot_written.is_none()
                || is_valid_tagged_value((self.value_written, self.pointer_tag_written)))
    }
}

#[derive(Default, Debug)]
pub struct StackPool {}

impl StackPool {
    pub fn get(&mut self) -> Stack {
        // A single instruction shouldn't be able to touch a new stack
        // but the stack is set to already written just in case.
        Stack {
            read: MockRead::new((U256::zero(), false)),
            slot_written: Some(45678),
            value_written: U256::zero(),
            pointer_tag_written: false,
        }
    }

    pub fn recycle(&mut self, _: Stack) {}
}
