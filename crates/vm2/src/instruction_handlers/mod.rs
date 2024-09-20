#[cfg(feature = "single_instruction_test")]
pub(crate) use ret::spontaneous_panic;

pub(crate) use self::{
    context::address_into_u256,
    heap_access::{AuxHeap, Heap},
    ret::invalid_instruction,
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
