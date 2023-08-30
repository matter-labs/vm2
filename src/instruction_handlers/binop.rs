use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, AnyDestination, AnySource, Arguments, Destination, DestinationWriter,
        Immediate1, Register1, Register2, RelativeStack, Source, SourceWriter,
    },
    predication::{Flags, Predicate},
    state::{Handler, Instruction, State},
};
use u256::U256;

fn binop<Op: Binop, In1: Source, Out: Destination, const SWAP: bool, const SET_FLAGS: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        let a = In1::get(args, state);
        let b = Register2::get(args, state);
        let (a, b) = if SWAP { (b, a) } else { (a, b) };

        let (result, out2, flags) = Op::perform(&a, &b);
        Out::set(args, state, result);
        out2.write(args, state);
        if SET_FLAGS {
            state.flags = flags;
        }
    });
}

pub trait Binop {
    type Out2: SecondOutput;
    fn perform(a: &U256, b: &U256) -> (U256, Self::Out2, Flags);
}

pub struct Add;
impl Binop for Add {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let (result, overflow) = a.overflowing_add(*b);
        (
            result,
            (),
            Flags::new(overflow, result.is_zero(), !(overflow || result.is_zero())),
        )
    }
    type Out2 = ();
}

pub struct Sub;
impl Binop for Sub {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let (result, overflow) = a.overflowing_sub(*b);
        (
            result,
            (),
            Flags::new(overflow, result.is_zero(), !(overflow || result.is_zero())),
        )
    }
    type Out2 = ();
}

pub struct And;
impl Binop for And {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a & *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct Or;
impl Binop for Or {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a | *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct Xor;
impl Binop for Xor {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a ^ *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct ShiftLeft;
impl Binop for ShiftLeft {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a << b.low_u32() as u8;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct ShiftRight;
impl Binop for ShiftRight {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a >> b.low_u32() as u8;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct RotateLeft;
impl Binop for RotateLeft {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let shift = b.low_u32() as u8;
        let result = *a << shift | *a >> (256 - shift as u16);
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub struct RotateRight;
impl Binop for RotateRight {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let shift = b.low_u32() as u8;
        let result = *a >> shift | *a << (256 - shift as u16);
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

pub trait SecondOutput {
    type Destination: DestinationWriter;
    fn write(self, args: &Arguments, state: &mut State);
}

impl SecondOutput for () {
    type Destination = ();
    fn write(self, _: &Arguments, _: &mut State) {}
}

impl DestinationWriter for () {
    fn write_destination(&self, _: &mut Arguments) {}
}

impl SecondOutput for U256 {
    type Destination = Register2;
    fn write(self, args: &Arguments, state: &mut State) {
        Self::Destination::set(args, state, self);
    }
}

pub struct Mul;
impl Binop for Mul {
    fn perform(a: &U256, b: &U256) -> (U256, Self::Out2, Flags) {
        let res = a.full_mul(*b);
        let (low_slice, high_slice) = res.0.split_at(4);

        let mut low_arr = [0; 4];
        low_arr.copy_from_slice(low_slice);
        let low = U256(low_arr);

        let mut high_arr = [0; 4];
        high_arr.copy_from_slice(high_slice);
        let high = U256(high_arr);

        (
            low,
            high,
            Flags::new(
                !high.is_zero(),
                low.is_zero(),
                high.is_zero() && !low.is_zero(),
            ),
        )
    }
    type Out2 = U256;
}

pub struct Div;
impl Binop for Div {
    fn perform(a: &U256, b: &U256) -> (U256, Self::Out2, Flags) {
        if *b != U256::zero() {
            let (quotient, remainder) = a.div_mod(*b);
            (
                quotient,
                remainder,
                Flags::new(false, !quotient.is_zero(), !remainder.is_zero()),
            )
        } else {
            (U256::zero(), U256::zero(), Flags::new(true, false, false)) // TODO check
        }
    }
    type Out2 = U256;
}

impl Instruction {
    #[inline(always)]
    pub fn from_binop<Op: Binop>(
        src1: AnySource,
        src2: Register2,
        out: AnyDestination,
        out2: <Op::Out2 as SecondOutput>::Destination,
        predicate: Predicate,
        swap: bool,
        set_flags: bool,
    ) -> Self {
        let mut arguments = Arguments::default();
        src1.write_source(&mut arguments);
        src2.write_source(&mut arguments);
        out.write_destination(&mut arguments);
        out2.write_destination(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: choose_binop_handler::<Op>(src1, out, swap, set_flags),
            arguments,
        }
    }
}

/// Maps run-time information to a monomorphized version of [binop].
#[inline(always)]
fn choose_binop_handler<Op: Binop>(
    input_type: AnySource,
    output_type: AnyDestination,
    swap: bool,
    set_flags: bool,
) -> Handler {
    match input_type {
        AnySource::Register1(_) => match_output_type::<Op, Register1>(output_type, swap, set_flags),
        AnySource::Immediate1(_) => {
            match_output_type::<Op, Immediate1>(output_type, swap, set_flags)
        }
        AnySource::AbsoluteStack(_) => {
            match_output_type::<Op, AbsoluteStack>(output_type, swap, set_flags)
        }
        AnySource::RelativeStack(_) => {
            match_output_type::<Op, RelativeStack>(output_type, swap, set_flags)
        }
    }
}

#[inline(always)]
fn match_output_type<Op: Binop, In1: Source>(
    output_type: AnyDestination,
    swap: bool,
    set_flags: bool,
) -> Handler {
    match output_type {
        AnyDestination::Register1(_) => match_swap::<Op, In1, Register1>(swap, set_flags),
        AnyDestination::AbsoluteStack(_) => match_swap::<Op, In1, AbsoluteStack>(swap, set_flags),
        AnyDestination::RelativeStack(_) => match_swap::<Op, In1, RelativeStack>(swap, set_flags),
    }
}

#[inline(always)]
fn match_swap<Op: Binop, In1: Source, Out: Destination>(swap: bool, set_flags: bool) -> Handler {
    if swap {
        match_set_flags::<Op, In1, Out, true>(set_flags)
    } else {
        match_set_flags::<Op, In1, Out, false>(set_flags)
    }
}

#[inline(always)]
fn match_set_flags<Op: Binop, In1: Source, Out: Destination, const SWAP: bool>(
    set_flags: bool,
) -> Handler {
    if set_flags {
        binop::<Op, In1, Out, SWAP, true>
    } else {
        binop::<Op, In1, Out, SWAP, false>
    }
}
