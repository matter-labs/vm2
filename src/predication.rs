const LT_BIT: u8 = 1;
const EQ_BIT: u8 = 1 << 1;
const GT_BIT: u8 = 1 << 2;
const ALWAYS_BIT: u8 = 1 << 3;

#[derive(Debug)]
pub struct Flags(u8);

impl Flags {
    pub fn new(lt_of: bool, eq: bool, gt: bool) -> Self {
        Flags(lt_of as u8 | ((eq as u8) << 1) | ((gt as u8) << 2) | ALWAYS_BIT)
    }
}

/// Predicate encoded so that comparing it to flags is efficient
#[derive(Copy, Clone, Debug, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u8)]
pub enum Predicate {
    Always = ALWAYS_BIT,
    IfGT = GT_BIT,
    IfEQ = EQ_BIT,
    IfLT = LT_BIT,
    IfGE = GT_BIT | EQ_BIT,
    IfLE = LT_BIT | EQ_BIT,
    IfNotEQ = EQ_BIT << 4 | ALWAYS_BIT,
    IfGtOrLT = GT_BIT | LT_BIT,
}

impl Predicate {
    #[inline(always)]
    pub fn satisfied(&self, flags: &Flags) -> bool {
        let bits = *self as u8;
        bits & flags.0 != 0 && (bits >> 4) & flags.0 == 0
    }
}

impl Default for Predicate {
    fn default() -> Self {
        Self::Always
    }
}
