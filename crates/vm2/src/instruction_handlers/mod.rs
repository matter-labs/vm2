pub use zksync_vm2_interface::opcodes::{
    Add, And, Div, Mul, Or, PointerAdd, PointerPack, PointerShrink, PointerSub, RotateLeft,
    RotateRight, ShiftLeft, ShiftRight, Sub, Xor,
};

pub(crate) use self::{
    heap_access::{AuxHeap, Heap},
    ret::{invalid_instruction, RETURN_COST},
};

mod binop;
mod common;
mod context;
mod decommit;
mod event;
mod far_call;
mod heap_access;
mod jump;
mod monomorphization;
mod near_call;
mod nop;
mod pointer;
mod precompiles;
mod ret;
mod storage;
