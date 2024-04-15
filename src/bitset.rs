#[derive(Clone)]
pub struct Bitset([u64; 1 << 10]);

impl Bitset {
    pub fn get(&self, i: u16) -> bool {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] & bit != 0
    }

    pub fn set(&mut self, i: u16) {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] |= bit;
    }

    pub fn clear(&mut self, i: u16) {
        let (slot, bit) = slot_and_bit(i);
        self.0[slot] &= !bit;
    }
}

fn slot_and_bit(i: u16) -> (usize, u64) {
    ((i >> 6) as usize, 1u64 << (i & 0b111111))
}

impl Default for Bitset {
    fn default() -> Self {
        Self([0; 1 << 10])
    }
}
