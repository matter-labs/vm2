use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register2, Source},
    Instruction, State,
};
use u256::U256;

fn pointer_op<Op: PointerOp, In: Source, Out: Destination, const SWAP: bool>(
    state: &mut State,
    instruction: *const Instruction,
) {
    instruction_boilerplate(state, instruction, |state, args| {
        let a = In::get(args, state);
        let b = Register2::get(args, state);
        let (a, b) = if SWAP { (b, a) } else { (a, b) };

        if let Some(result) = Op::perform(&a, &b) {
            Out::set(args, state, result);
        } else {
            // TODO panic
        }
    });
}

trait PointerOp {
    fn perform(pointer: &U256, offset: &U256) -> Option<U256>;
}

struct Add;
impl PointerOp for Add {
    fn perform(pointer: &U256, offset: &U256) -> Option<U256> {
        let offset: u32 = try_u256_into_u32(offset)?;
        let mut result = pointer.clone();
        let offset_field = get_offset_mut(&mut result);
        *offset_field = offset_field.checked_add(offset)?;
        Some(result)
    }
}

#[cfg(target_endian = "little")]
fn get_offset_mut(fat_pointer: &mut U256) -> &mut u32 {
    unsafe { &mut *(fat_pointer as *mut U256 as *mut u32) }
}

#[cfg(target_endian = "little")]
fn get_length_mut(fat_pointer: &mut U256) -> &mut u32 {
    unsafe { &mut (&mut *(fat_pointer as *mut U256 as *mut [u32; 8]))[3] }
}

fn try_u256_into_u32(x: &U256) -> Option<u32> {
    if x < &U256([u32::MAX as u64, 0, 0, 0]) {
        Some(x.low_u32())
    } else {
        None
    }
}

/*
impl Instruction {
    pub fn from_ptr_add() -> Self {
        Self {
            handler: pointer_op::<Add>,
            arguments: Arguments::default(),
        }
    }
}*/
