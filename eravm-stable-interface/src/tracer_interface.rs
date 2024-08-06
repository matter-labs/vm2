use crate::StateInterface;

pub trait Tracer<Opcode> {
    #[inline(always)]
    fn before_instruction<S: StateInterface>(&mut self, _state: &mut S) {}

    #[inline(always)]
    fn after_instruction<S: StateInterface>(&mut self, _state: &mut S) {}
}

// The &mut is a workaround for the lack of specialization in stable Rust.
// Trait resolution will choose an implementation on U over the implementation on `&mut U`.
impl<T, U> Tracer<T> for &mut U {}

// These are all the opcodes the VM currently supports.
// Old tracers will keep working even if new opcodes are added.
// The Tracer trait itself never needs to be updated.
pub mod opcodes {
    pub struct Nop;
    pub struct Add;
    pub struct Sub;
    pub struct And;
    pub struct Or;
    pub struct Xor;
    pub struct ShiftLeft;
    pub struct ShiftRight;
    pub struct RotateLeft;
    pub struct RotateRight;
    pub struct Mul;
    pub struct Div;
    pub struct NearCall;
    pub struct FarCall;
    pub struct Ret;
    pub struct Jump;
    pub struct Event;
    pub struct L2ToL1Message;
    pub struct Decommit;
    pub struct This;
    pub struct Caller;
    pub struct CodeAddress;
    pub struct ErgsLeft;
    pub struct U128;
    pub struct SP;
    pub struct ContextMeta;
    pub struct SetContextU128;
    pub struct IncrementTxNumber;
    pub struct AuxMutating0;
    pub struct PrecompileCall;
    pub struct HeapRead;
    pub struct HeapWrite;
    pub struct PointerRead;
    pub struct PointerAdd;
    pub struct PointerSub;
    pub struct PointerPack;
    pub struct PointerShrink;
    pub struct StorageRead;
    pub struct StorageWrite;
    pub struct TransientStorageRead;
    pub struct TransientStorageWrite;
    pub struct StaticMemoryRead;
    pub struct StaticMemoryWrite;
}
