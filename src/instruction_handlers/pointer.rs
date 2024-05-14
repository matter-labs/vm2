use super::{common::instruction_boilerplate_with_panic, PANIC};
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnyDestination, AnySource, Arguments, CodePage,
        Destination, Immediate1, Register1, Register2, RelativeStack, Source,
    },
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    Instruction, VirtualMachine,
};
use u256::U256;

fn ptr<Op: PtrOp, In1: Source, Out: Destination, const SWAP: bool>(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        let a = (
            In1::get(args, &mut vm.state),
            In1::is_fat_pointer(args, &mut vm.state),
        );
        let b = (
            Register2::get(args, &mut vm.state),
            Register2::is_fat_pointer(args, &mut vm.state),
        );
        let (a, b) = if SWAP { (b, a) } else { (a, b) };
        let (a, a_is_pointer) = a;
        let (b, b_is_pointer) = b;

        if !a_is_pointer || b_is_pointer {
            return Ok(&PANIC);
        }

        let Some(result) = Op::perform(a, b) else {
            return Ok(&PANIC);
        };

        Out::set_fat_ptr(args, &mut vm.state, result);

        continue_normally
    })
}

pub trait PtrOp {
    fn perform(in1: U256, in2: U256) -> Option<U256>;
}

pub struct PtrAddSub<const IS_ADD: bool>;
pub type PtrAdd = PtrAddSub<true>;
pub type PtrSub = PtrAddSub<false>;

impl<const IS_ADD: bool> PtrOp for PtrAddSub<IS_ADD> {
    fn perform(mut in1: U256, in2: U256) -> Option<U256> {
        if in2 > u32::MAX.into() {
            return None;
        }
        let pointer: &mut FatPointer = (&mut in1).into();

        let new_offset = if IS_ADD {
            pointer.offset.checked_add(in2.low_u32())
        } else {
            pointer.offset.checked_sub(in2.low_u32())
        }?;

        pointer.offset = new_offset;

        Some(in1)
    }
}

pub struct PtrPack;
impl PtrOp for PtrPack {
    fn perform(in1: U256, in2: U256) -> Option<U256> {
        if in2.low_u128() != 0 {
            None
        } else {
            Some(U256([in1.0[0], in1.0[1], in2.0[2], in2.0[3]]))
        }
    }
}

pub struct PtrShrink;
impl PtrOp for PtrShrink {
    fn perform(mut in1: U256, in2: U256) -> Option<U256> {
        let pointer: &mut FatPointer = (&mut in1).into();
        pointer.length = pointer.length.checked_sub(in2.low_u32())?;
        Some(in1)
    }
}

use super::monomorphization::*;

impl Instruction {
    #[inline(always)]
    pub fn from_ptr<Op: PtrOp>(
        src1: AnySource,
        src2: Register2,
        out: AnyDestination,
        arguments: Arguments,
        swap: bool,
    ) -> Self {
        Self {
            handler: monomorphize!(ptr [Op] match_source src1 match_destination out match_boolean swap),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_destination(&out),
        }
    }
}
