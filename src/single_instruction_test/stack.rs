use super::{mock_array::MockRead, validation::is_valid_tagged_value};
use arbitrary::Arbitrary;
use u256::U256;

#[derive(PartialEq, Debug, Clone)]
pub struct Stack {
    read: MockRead<u16, (U256, bool)>,
    slot_written: Option<u16>,
    value_written: U256,
    pointer_tag_written: bool,
}

impl<'a> Arbitrary<'a> for Stack {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            read: u.arbitrary()?,
            slot_written: None,
            value_written: 0.into(),
            pointer_tag_written: false,
        })
    }
}

impl Stack {
    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.read.get(slot).0
    }

    pub(crate) fn get_pointer_flag(&self, slot: u16) -> bool {
        self.read.get(slot).1
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256) {
        self.assert_write_to_same_slot(slot);
        self.value_written = value;
    }

    pub(crate) fn set_pointer_flag(&mut self, slot: u16) {
        self.assert_write_to_same_slot(slot);
        self.pointer_tag_written = true;
    }

    pub(crate) fn clear_pointer_flag(&mut self, slot: u16) {
        self.assert_write_to_same_slot(slot);
        self.pointer_tag_written = false;
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

#[derive(Default, Debug, Arbitrary)]
pub struct StackPool(Option<Stack>);

impl StackPool {
    pub fn get(&mut self) -> Box<Stack> {
        Box::new(self.0.take().unwrap())
    }

    pub fn recycle(&mut self, _: Box<Stack>) {}
}
