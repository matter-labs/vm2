use crate::state::Instruction;
use arbitrary::Arbitrary;
pub use binop::{Add, And, Div, Mul, Or, RotateLeft, RotateRight, ShiftLeft, ShiftRight, Sub, Xor};
pub use pointer::{PtrAdd, PtrPack, PtrShrink, PtrSub};

mod binop;
mod common;
mod counter;
mod jump;
mod nop;
mod pointer;

impl<'a> Arbitrary<'a> for Instruction {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let predicate = u.arbitrary()?;
        let swap: bool = u.arbitrary()?;
        let set_flags = u.arbitrary()?;
        let src1 = u.arbitrary()?;
        let src2 = u.arbitrary()?;
        let out = u.arbitrary()?;

        Ok(match u.choose_index(12)? {
            0 => Self::from_binop::<Add>(src1, src2, out, (), predicate, swap, set_flags),
            1 => Self::from_binop::<Sub>(src1, src2, out, (), predicate, swap, set_flags),
            2 => Self::from_binop::<And>(src1, src2, out, (), predicate, swap, set_flags),
            3 => Self::from_binop::<Or>(src1, src2, out, (), predicate, swap, set_flags),
            4 => Self::from_binop::<Xor>(src1, src2, out, (), predicate, swap, set_flags),
            5 => Self::from_binop::<Xor>(src1, src2, out, (), predicate, swap, set_flags),
            6 => Self::from_binop::<ShiftLeft>(src1, src2, out, (), predicate, swap, set_flags),
            7 => Self::from_binop::<ShiftRight>(src1, src2, out, (), predicate, swap, set_flags),
            8 => Self::from_binop::<RotateLeft>(src1, src2, out, (), predicate, swap, set_flags),
            9 => Self::from_binop::<RotateRight>(src1, src2, out, (), predicate, swap, set_flags),
            10 => {
                Self::from_binop::<Mul>(src1, src2, out, u.arbitrary()?, predicate, swap, set_flags)
            }
            11 => {
                Self::from_binop::<Div>(src1, src2, out, u.arbitrary()?, predicate, swap, set_flags)
            }
            _ => unreachable!(),
        })
    }
}
