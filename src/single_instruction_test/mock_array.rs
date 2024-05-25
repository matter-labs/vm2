use arbitrary::Arbitrary;
use std::cell::Cell;
use std::fmt::Debug;

#[derive(Clone, Debug)]
pub struct MockRead<I: Copy, T> {
    pub(crate) value_read: T,
    index_read: Cell<Option<I>>,
}

impl<I: PartialEq + Copy + Debug, T: Clone> MockRead<I, T> {
    pub fn new(value: T) -> Self {
        Self {
            value_read: value,
            index_read: Cell::new(None),
        }
    }

    pub fn get(&self, index: I) -> &T {
        if let Some(previous_index) = self.index_read.get() {
            assert_eq!(previous_index, index);
        }
        self.index_read.set(Some(index));

        &self.value_read
    }

    pub fn get_mut(&mut self, index: I) -> &mut T {
        if let Some(previous_index) = self.index_read.get() {
            assert_eq!(previous_index, index);
        }
        self.index_read.set(Some(index));

        &mut self.value_read
    }

    pub fn read_that_happened(&self) -> Option<(I, T)> {
        self.index_read
            .get()
            .map(|index| (index, self.value_read.clone()))
    }
}

impl<'a, I: PartialEq + Copy + Debug, T: Clone + Arbitrary<'a>> Arbitrary<'a> for MockRead<I, T> {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::new(u.arbitrary()?))
    }
}

impl<I: Copy, T> PartialEq for MockRead<I, T> {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
