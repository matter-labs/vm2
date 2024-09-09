/// Bitset with `1 << 16` elements. Used to store pointer flags for VM [`Stack`](crate::stack::Stack).
#[derive(Clone, PartialEq, Debug, Hash)]
pub(crate) struct Bitset([u64; 1 << 10]);

impl Bitset {
    #[inline(always)]
    pub fn get(&self, i: u16) -> bool {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] & bit != 0
    }

    #[inline(always)]
    pub fn set(&mut self, i: u16) {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] |= bit;
    }

    #[inline(always)]
    pub fn clear(&mut self, i: u16) {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] &= !bit;
    }
}

#[inline(always)]
fn slot_and_bit(i: u16) -> (usize, u64) {
    ((i >> 6) as usize, 1u64 << (i & 0b111111))
}

impl Default for Bitset {
    fn default() -> Self {
        Self([0; 1 << 10])
    }
}
