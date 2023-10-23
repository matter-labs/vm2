use crate::{state::Instruction, Predicate};
use arbitrary::Arbitrary;
pub use binop::{Add, And, Div, Mul, Or, RotateLeft, RotateRight, ShiftLeft, ShiftRight, Sub, Xor};
pub use heap_access::{AuxHeap, Heap};
pub use pointer::{PtrAdd, PtrPack, PtrShrink, PtrSub};

mod binop;
mod common;
mod counter;
mod heap_access;
mod jump;
mod monomorphization;
mod nop;
mod pointer;

impl<'a> Arbitrary<'a> for Instruction {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let predicate = if u.arbitrary()? {
            Predicate::Always
        } else {
            u.arbitrary()?
        };
        let swap: bool = u.arbitrary()?;
        let set_flags = u.arbitrary()?;
        let src1 = u.arbitrary()?;
        let src2 = u.arbitrary()?;
        let out = u.arbitrary()?;

        Ok(match u.choose_index(19)? {
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
            12 => Self::from_ptr::<PtrAdd>(src1, src2, out, predicate, swap),
            13 => Self::from_ptr::<PtrSub>(src1, src2, out, predicate, swap),
            14 => Self::from_ptr::<PtrShrink>(src1, src2, out, predicate, swap),
            15 => Self::from_ptr::<PtrPack>(src1, src2, out, predicate, swap),
            16 => {
                Self::from_load_pointer(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?, predicate)
            }
            17 => {
                Self::from_load::<Heap>(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?, predicate)
            }
            18 => {
                Self::from_store::<Heap>(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?, predicate)
            }
            _ => unreachable!(),
        })
    }
}
