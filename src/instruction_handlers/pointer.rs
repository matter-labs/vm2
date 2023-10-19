use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnyDestination, AnySource, Arguments, CodePage,
        Destination, DestinationWriter, Immediate1, Register1, Register2, RelativeStack, Source,
        SourceWriter,
    },
    state::Handler,
    Instruction, Predicate, State,
};
use u256::U256;

#[repr(C)]
struct FatPointer {
    offset: u32,
    memory_page: u32,
    start: u32,
    length: u32,
}

#[cfg(target_endian = "little")]
impl From<&mut U256> for &mut FatPointer {
    fn from(value: &mut U256) -> Self {
        unsafe { &mut *(value as *mut U256).cast() }
    }
}

fn ptr<Op: PtrOp, In1: Source, Out: Destination, const SWAP: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        let a = (In1::get(args, state), In1::is_fat_pointer(args, state));
        let b = (
            Register2::get(args, state),
            Register2::is_fat_pointer(args, state),
        );
        let (a, b) = if SWAP { (b, a) } else { (a, b) };
        let (a, a_is_pointer) = a;
        let (b, b_is_pointer) = b;

        if a_is_pointer && !b_is_pointer {
            let result = Op::perform(a, b);
            Out::set_fat_ptr(args, state, result);
        } else {
            // TODO panic
        }
    });
}

pub trait PtrOp {
    fn perform(in1: U256, in2: U256) -> U256;
}

pub struct PtrAddSub<const IS_ADD: bool>;
pub type PtrAdd = PtrAddSub<true>;
pub type PtrSub = PtrAddSub<false>;

impl<const IS_ADD: bool> PtrOp for PtrAddSub<IS_ADD> {
    fn perform(mut in1: U256, in2: U256) -> U256 {
        if in2 > u32::MAX.into() {
            // TODO panic
        }
        let pointer: &mut FatPointer = (&mut in1).into();

        let (new_offset, overflowed) = if IS_ADD {
            pointer.offset.overflowing_add(in2.low_u32())
        } else {
            pointer.offset.overflowing_sub(in2.low_u32())
        };
        if overflowed {
            // TODO panic
        }
        pointer.offset = new_offset;

        in1
    }
}

pub struct PtrPack;
impl PtrOp for PtrPack {
    fn perform(in1: U256, in2: U256) -> U256 {
        if in2.low_u128() != 0 {
            // TODO panic
        }
        U256([in1.0[0], in1.0[1], in2.0[2], in2.0[3]])
    }
}

pub struct PtrShrink;
impl PtrOp for PtrShrink {
    fn perform(mut in1: U256, in2: U256) -> U256 {
        let pointer: &mut FatPointer = (&mut in1).into();
        let (new_len, overflowed) = pointer.length.overflowing_sub(in2.low_u32());
        if overflowed {
            // TODO panic
        }
        pointer.length = new_len;
        in1
    }
}

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
            handler: choose_ptr_handler::<Op>(src1, out, swap),
            arguments,
        }
    }
}

/// Maps run-time information to a monomorphized version of [binop].
#[inline(always)]
fn choose_ptr_handler<Op: PtrOp>(
    input_type: AnySource,
    output_type: AnyDestination,
    swap: bool,
) -> Handler {
    match input_type {
        AnySource::Register1(_) => match_output_type::<Op, Register1>(output_type, swap),
        AnySource::Immediate1(_) => match_output_type::<Op, Immediate1>(output_type, swap),
        AnySource::AbsoluteStack(_) => match_output_type::<Op, AbsoluteStack>(output_type, swap),
        AnySource::RelativeStack(_) => match_output_type::<Op, RelativeStack>(output_type, swap),
        AnySource::AdvanceStackPointer(_) => {
            match_output_type::<Op, AdvanceStackPointer>(output_type, swap)
        }
        AnySource::CodePage(_) => match_output_type::<Op, CodePage>(output_type, swap),
    }
}

#[inline(always)]
fn match_output_type<Op: PtrOp, In1: Source>(output_type: AnyDestination, swap: bool) -> Handler {
    match output_type {
        AnyDestination::Register1(_) => match_swap::<Op, In1, Register1>(swap),
        AnyDestination::AbsoluteStack(_) => match_swap::<Op, In1, AbsoluteStack>(swap),
        AnyDestination::RelativeStack(_) => match_swap::<Op, In1, RelativeStack>(swap),
        AnyDestination::AdvanceStackPointer(_) => match_swap::<Op, In1, AdvanceStackPointer>(swap),
    }
}

#[inline(always)]
fn match_swap<Op: PtrOp, In1: Source, Out: Destination>(swap: bool) -> Handler {
    if swap {
        ptr::<Op, In1, Out, true>
    } else {
        ptr::<Op, In1, Out, false>
    }
}
