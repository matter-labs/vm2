use crate::StateInterface;

pub mod opcodes {
    pub struct Nop;
    pub struct NearCall;
    pub struct FarCall;
    pub struct Ret;
    pub struct Jump;
    pub struct Event;
    pub struct L2ToL1Message;
    pub struct Decommit;
    pub struct ContextMeta;
    pub struct SetContextU128;
    pub struct IncrementTxNumber;
    pub struct AuxMutating0;
    pub struct PrecompileCall;
    pub struct HeapRead;
    pub struct HeapWrite;
    pub struct PointerRead;
    pub struct StorageRead;
    pub struct StorageWrite;
    pub struct TransientStorageRead;
    pub struct TransientStorageWrite;
    pub struct StaticMemoryRead;
    pub struct StaticMemoryWrite;
}

pub trait Tracer<Opcode> {
    #[inline(always)]
    fn before_instruction<S: StateInterface>(&mut self, _state: &mut S) {}

    #[inline(always)]
    fn after_instruction<S: StateInterface>(&mut self, _state: &mut S) {}
}

// The &mut is a workaround for the lack of specialization in stable Rust.
// Trait resolution will choose an implementation on U over the implementation on `&mut U`.
impl<T, U> Tracer<T> for &mut U {}
