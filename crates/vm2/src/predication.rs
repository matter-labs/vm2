const LT_BIT: u8 = 1;
const EQ_BIT: u8 = 1 << 1;
const GT_BIT: u8 = 1 << 2;
const ALWAYS_BIT: u8 = 1 << 3;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Flags(u8);

impl Flags {
    pub(crate) fn new(lt_of: bool, eq: bool, gt: bool) -> Self {
        Flags(u8::from(lt_of) | (u8::from(eq) << 1) | (u8::from(gt) << 2) | ALWAYS_BIT)
    }
}

/// Predicate for an instruction. Encoded so that comparing it to flags is efficient.
#[derive(Copy, Clone, Debug, Default, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u8)]
pub enum Predicate {
    /// Always execute the associated instruction.
    #[default]
    Always = ALWAYS_BIT,
    /// Execute the associated instruction if the "greater than" execution flag is set.
    IfGT = GT_BIT,
    /// Execute the associated instruction if the "equal" execution flag is set.
    IfEQ = EQ_BIT,
    /// Execute the associated instruction if the "less than" execution flag is set.
    IfLT = LT_BIT,
    /// Execute the associated instruction if either of "greater than" or "equal" execution flags are set.
    IfGE = GT_BIT | EQ_BIT,
    /// Execute the associated instruction if either of "less than" or "equal" execution flags are set.
    IfLE = LT_BIT | EQ_BIT,
    /// Execute the associated instruction if the "equal" execution flag is not set.
    IfNotEQ = EQ_BIT << 4 | ALWAYS_BIT,
    /// Execute the associated instruction if either of "less than" or "greater than" execution flags are set.
    IfGTOrLT = GT_BIT | LT_BIT,
}

impl Predicate {
    #[inline(always)]
    pub(crate) fn satisfied(self, flags: &Flags) -> bool {
        let bits = self as u8;
        bits & flags.0 != 0 && (bits >> 4) & flags.0 == 0
    }
}

#[cfg(feature = "single_instruction_test")]
impl From<&Flags> for zk_evm::flags::Flags {
    fn from(flags: &Flags) -> Self {
        zk_evm::flags::Flags {
            overflow_or_less_than_flag: flags.0 & LT_BIT != 0,
            equality_flag: flags.0 & EQ_BIT != 0,
            greater_than_flag: flags.0 & GT_BIT != 0,
        }
    }
}
