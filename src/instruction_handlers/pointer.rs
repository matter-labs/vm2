use super::common::run_next_instruction;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnyDestination, AnySource, Arguments, CodePage,
        Destination, DestinationWriter, Immediate1, Register1, Register2, RelativeStack, Source,
        SourceWriter,
    },
    fat_pointer::FatPointer,
    state::{ExecutionResult, Panic},
    Instruction, Predicate, State,
};
use u256::U256;

fn ptr<Op: PtrOp, In1: Source, Out: Destination, const SWAP: bool>(
    state: &mut State,
    instruction: *const Instruction,
) -> ExecutionResult {
    let args = unsafe { &(*instruction).arguments };

    let a = (In1::get(args, state), In1::is_fat_pointer(args, state));
    let b = (
        Register2::get(args, state),
        Register2::is_fat_pointer(args, state),
    );
    let (a, b) = if SWAP { (b, a) } else { (a, b) };
    let (a, a_is_pointer) = a;
    let (b, b_is_pointer) = b;

    if !a_is_pointer || b_is_pointer {
        return Err(Panic::IncorrectPointerTags);
    }

    let result = Op::perform(a, b)?;

    Out::set_fat_ptr(args, state, result);

    run_next_instruction(state, instruction)
}

pub trait PtrOp {
    fn perform(in1: U256, in2: U256) -> Result<U256, Panic>;
}

pub struct PtrAddSub<const IS_ADD: bool>;
pub type PtrAdd = PtrAddSub<true>;
pub type PtrSub = PtrAddSub<false>;

impl<const IS_ADD: bool> PtrOp for PtrAddSub<IS_ADD> {
    fn perform(mut in1: U256, in2: U256) -> Result<U256, Panic> {
        if in2 > u32::MAX.into() {
            return Err(Panic::PointerOffsetTooLarge);
        }
        let pointer: &mut FatPointer = (&mut in1).into();

        let new_offset = if IS_ADD {
            pointer.offset.checked_add(in2.low_u32())
        } else {
            pointer.offset.checked_sub(in2.low_u32())
        }
        .ok_or(Panic::PointerOffsetTooLarge)?;

        pointer.offset = new_offset;

        Ok(in1)
    }
}

pub struct PtrPack;
impl PtrOp for PtrPack {
    fn perform(in1: U256, in2: U256) -> Result<U256, Panic> {
        if in2.low_u128() != 0 {
            Err(Panic::PtrPackLowBitsNotZero)
        } else {
            Ok(U256([in1.0[0], in1.0[1], in2.0[2], in2.0[3]]))
        }
    }
}

pub struct PtrShrink;
impl PtrOp for PtrShrink {
    fn perform(mut in1: U256, in2: U256) -> Result<U256, Panic> {
        let pointer: &mut FatPointer = (&mut in1).into();
        pointer.length = pointer
            .length
            .checked_sub(in2.low_u32())
            .ok_or(Panic::PointerOffsetTooLarge)?;
        Ok(in1)
    }
}

use super::monomorphization::*;

impl Instruction {
    #[inline(always)]
    pub fn from_ptr<Op: PtrOp>(
        src1: AnySource,
        src2: Register2,
        out: AnyDestination,
        predicate: Predicate,
        swap: bool,
    ) -> Self {
        let mut arguments = Arguments::default();
        src1.write_source(&mut arguments);
        src2.write_source(&mut arguments);
        out.write_destination(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: monomorphize!(ptr [Op] match_source src1 match_destination out match_boolean swap),
            arguments,
        }
    }
}
