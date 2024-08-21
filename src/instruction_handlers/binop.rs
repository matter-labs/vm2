use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{
        AbsoluteStack, Addressable, AdvanceStackPointer, AnyDestination, AnySource, Arguments,
        CodePage, Destination, DestinationWriter, Immediate1, Register1, Register2, RelativeStack,
        Source,
    },
    instruction::{ExecutionStatus, Instruction},
    predication::Flags,
    VirtualMachine,
};
use eravm_stable_interface::{
    opcodes::{Add, And, Div, Mul, Or, RotateLeft, RotateRight, ShiftLeft, ShiftRight, Sub, Xor},
    OpcodeType, Tracer,
};
use u256::U256;

fn binop<
    T: Tracer,
    W,
    Op: Binop,
    In1: Source,
    Out: Destination,
    const SWAP: bool,
    const SET_FLAGS: bool,
>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<Op, _, _>(vm, world, tracer, |vm, args, _| {
        let a = In1::get(args, &mut vm.state);
        let b = Register2::get(args, &mut vm.state);
        let (a, b) = if SWAP { (b, a) } else { (a, b) };

        let (result, out2, flags) = Op::perform(&a, &b);
        Out::set(args, &mut vm.state, result);
        out2.write(args, &mut vm.state);
        if SET_FLAGS {
            vm.state.flags = flags;
        }
    })
}

pub trait Binop: OpcodeType {
    type Out2: SecondOutput;
    fn perform(a: &U256, b: &U256) -> (U256, Self::Out2, Flags);
}

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

impl Binop for And {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a & *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

impl Binop for Or {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a | *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

impl Binop for Xor {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a ^ *b;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

impl Binop for ShiftLeft {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a << b.low_u32() as u8;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

impl Binop for ShiftRight {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let result = *a >> b.low_u32() as u8;
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

impl Binop for RotateLeft {
    #[inline(always)]
    fn perform(a: &U256, b: &U256) -> (U256, (), Flags) {
        let shift = b.low_u32() as u8;
        let result = *a << shift | *a >> (256 - shift as u16);
        (result, (), Flags::new(false, result.is_zero(), false))
    }
    type Out2 = ();
}

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
    fn write(self, args: &Arguments, state: &mut impl Addressable);
}

impl SecondOutput for () {
    type Destination = ();
    fn write(self, _: &Arguments, _: &mut impl Addressable) {}
}

impl DestinationWriter for () {
    fn write_destination(&self, _: &mut Arguments) {}
}

impl SecondOutput for U256 {
    type Destination = Register2;
    fn write(self, args: &Arguments, state: &mut impl Addressable) {
        Self::Destination::set(args, state, self);
    }
}

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

impl Binop for Div {
    fn perform(a: &U256, b: &U256) -> (U256, Self::Out2, Flags) {
        if *b != U256::zero() {
            let (quotient, remainder) = a.div_mod(*b);
            (
                quotient,
                remainder,
                Flags::new(false, quotient.is_zero(), remainder.is_zero()),
            )
        } else {
            (U256::zero(), U256::zero(), Flags::new(true, false, false))
        }
    }
    type Out2 = U256;
}

use super::monomorphization::*;

impl<T: Tracer, W> Instruction<T, W> {
    #[inline(always)]
    pub fn from_binop<Op: Binop>(
        src1: AnySource,
        src2: Register2,
        out: AnyDestination,
        out2: <Op::Out2 as SecondOutput>::Destination,
        arguments: Arguments,
        swap: bool,
        set_flags: bool,
    ) -> Self {
        Self {
            handler: monomorphize!(binop [T W Op] match_source src1 match_destination out match_boolean swap match_boolean set_flags),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_destination(&out)
                .write_destination(&out2),
        }
    }
}
