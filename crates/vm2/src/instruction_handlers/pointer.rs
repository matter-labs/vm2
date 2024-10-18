use primitive_types::U256;
use zksync_vm2_interface::{
    opcodes::{PointerAdd, PointerPack, PointerShrink, PointerSub},
    OpcodeType, Tracer,
};

use super::{
    common::boilerplate,
    monomorphization::{
        match_boolean, match_destination, match_source, monomorphize, parameterize,
    },
    ret::spontaneous_panic,
};
use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnyDestination, AnySource, Arguments, CodePage,
        Destination, Immediate1, Register1, Register2, RelativeStack, Source,
    },
    fat_pointer::FatPointer,
    instruction::ExecutionStatus,
    Instruction, VirtualMachine, World,
};

fn ptr<T: Tracer, W: World<T>, Op: PtrOp, In1: Source, Out: Destination, const SWAP: bool>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate::<Op, _, _>(vm, world, tracer, |vm, args| {
        let ((a, a_is_pointer), (b, b_is_pointer)) = if SWAP {
            (
                Register2::get_with_pointer_flag(args, &mut vm.state),
                In1::get_with_pointer_flag_and_erasing(args, &mut vm.state),
            )
        } else {
            (
                In1::get_with_pointer_flag(args, &mut vm.state),
                Register2::get_with_pointer_flag_and_erasing(args, &mut vm.state),
            )
        };

        if !a_is_pointer || b_is_pointer {
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        }

        let Some(result) = Op::perform(a, b) else {
            vm.state.current_frame.pc = spontaneous_panic();
            return;
        };

        Out::set_fat_ptr(args, &mut vm.state, result);
    })
}

pub(crate) trait PtrOp: OpcodeType {
    fn perform(in1: U256, in2: U256) -> Option<U256>;
}

impl PtrOp for PointerAdd {
    #[inline(always)]
    fn perform(in1: U256, in2: U256) -> Option<U256> {
        ptr_add_sub::<true>(in1, in2)
    }
}

impl PtrOp for PointerSub {
    #[inline(always)]
    fn perform(in1: U256, in2: U256) -> Option<U256> {
        ptr_add_sub::<false>(in1, in2)
    }
}

fn ptr_add_sub<const IS_ADD: bool>(mut in1: U256, in2: U256) -> Option<U256> {
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

impl PtrOp for PointerPack {
    #[inline(always)]
    fn perform(in1: U256, in2: U256) -> Option<U256> {
        if in2.low_u128() != 0 {
            None
        } else {
            Some(U256([in1.0[0], in1.0[1], in2.0[2], in2.0[3]]))
        }
    }
}

impl PtrOp for PointerShrink {
    #[inline(always)]
    fn perform(mut in1: U256, in2: U256) -> Option<U256> {
        let pointer: &mut FatPointer = (&mut in1).into();
        pointer.length = pointer.length.checked_sub(in2.low_u32())?;
        Some(in1)
    }
}

macro_rules! from_ptr_op {
    ($name:ident <$binop:ty>) => {
        #[doc = concat!("Creates a [`", stringify!($binop), "`] instruction with the provided params.")]
        pub fn $name(
            src1: AnySource,
            src2: Register2,
            out: AnyDestination,
            arguments: Arguments,
            swap: bool,
        ) -> Self {
            Self::from_ptr::<$binop>(src1, src2, out, arguments, swap)
        }
    };
}

/// Pointer-related instructions.
impl<T: Tracer, W: World<T>> Instruction<T, W> {
    from_ptr_op!(from_pointer_add<PointerAdd>);
    from_ptr_op!(from_pointer_sub<PointerSub>);
    from_ptr_op!(from_pointer_pack<PointerPack>);
    from_ptr_op!(from_pointer_shrink<PointerShrink>);

    pub(crate) fn from_ptr<Op: PtrOp>(
        src1: AnySource,
        src2: Register2,
        out: AnyDestination,
        arguments: Arguments,
        swap: bool,
    ) -> Self {
        Self {
            handler: monomorphize!(ptr [T W Op] match_source src1 match_destination out match_boolean swap),
            arguments: arguments
                .write_source(&src1)
                .write_source(&src2)
                .write_destination(&out),
        }
    }
}
