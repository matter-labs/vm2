use arbitrary::Arbitrary;
use u256::U256;

use super::mock_array::MockRead;

#[derive(PartialEq, Debug, Clone)]
pub struct Stack {
    read: MockRead<u16, (U256, bool)>,
    slot_written: Option<u16>,
}

impl<'a> Arbitrary<'a> for Stack {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            read: u.arbitrary()?,
            slot_written: None,
        })
    }
}

impl Stack {
    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.read.get(slot).0
    }

    pub(crate) fn set(&mut self, slot: u16, _: U256) {
        self.slot_written = Some(slot);
    }

    pub(crate) fn get_pointer_flag(&self, slot: u16) -> bool {
        self.read.get(slot).1
    }

    pub(crate) fn set_pointer_flag(&mut self, slot: u16) {
        self.slot_written = Some(slot);
    }

    pub(crate) fn clear_pointer_flag(&mut self, slot: u16) {
        self.slot_written = Some(slot);
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
