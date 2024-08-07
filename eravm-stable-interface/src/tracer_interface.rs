use crate::StateInterface;
use std::marker::PhantomData;

/// For example, if you want `FarCallCounter` to trace far calls,
/// you need to implement [Tracer<FarCall>] for `FarCallCounter`.
/// ```
/// use eravm_stable_interface::{Tracer, opcodes, StateInterface};
/// struct FarCallCounter(usize);
/// impl Tracer<opcodes::FarCall> for FarCallCounter {
///     fn before_instruction<S: StateInterface>(&mut self, state: &mut S) {
///         self.0 += 1;
///     }
/// }
/// ```
pub trait Tracer<Opcode> {
    #[inline(always)]
    fn before_instruction<S: StateInterface>(&mut self, _state: &mut S) {}

    #[inline(always)]
    fn after_instruction<S: StateInterface>(&mut self, _state: &mut S) {}
}

/// This trait is a workaround for the lack of specialization in stable Rust.
/// Trait resolution will choose an implementation on U over the implementation on `&U`.
///
/// Useful for VM implementers only.
/// If you import this trait and [OpcodeSelect], you can notify a tracer like this:
/// `tracer.opcode::<opcodes::Div>().before_instruction(&mut state)`
pub trait TracerDispatch {
    fn before_instruction<S: StateInterface>(self, state: &mut S);
    fn after_instruction<S: StateInterface>(self, state: &mut S);
}

pub struct TraceCase<T, I> {
    tracer: T,
    _opcode: PhantomData<I>,
}

impl<T, I> TracerDispatch for TraceCase<&mut T, I>
where
    T: Tracer<I>,
{
    fn before_instruction<S: StateInterface>(self, state: &mut S) {
        self.tracer.before_instruction(state);
    }

    fn after_instruction<S: StateInterface>(self, state: &mut S) {
        self.tracer.after_instruction(state);
    }
}

impl<T, I> TracerDispatch for &TraceCase<&mut T, I> {
    #[inline(always)]
    fn before_instruction<S: StateInterface>(self, _: &mut S) {}
    #[inline(always)]
    fn after_instruction<S: StateInterface>(self, _: &mut S) {}
}

/// To be used with [TracerDispatch].
pub trait OpcodeSelect {
    fn opcode<I>(&mut self) -> TraceCase<&mut Self, I>;
}

impl<T> OpcodeSelect for T {
    fn opcode<I>(&mut self) -> TraceCase<&mut Self, I> {
        TraceCase {
            tracer: self,
            _opcode: PhantomData::<I>,
        }
    }
}

// Multiple tracers can be combined by building a linked list out of tuples.
impl<A, B, I> Tracer<I> for (A, B)
where
    A: Tracer<I>,
    B: Tracer<I>,
{
    fn before_instruction<S: crate::StateInterface>(&mut self, state: &mut S) {
        self.0.before_instruction(state);
        self.1.before_instruction(state);
    }

    fn after_instruction<S: crate::StateInterface>(&mut self, state: &mut S) {
        self.0.after_instruction(state);
        self.1.after_instruction(state);
    }
}

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

#[cfg(test)]
mod tests {
    use crate::{opcodes, DummyState, OpcodeSelect, Tracer, TracerDispatch};

    struct FarCallCounter(usize);

    impl Tracer<opcodes::FarCall> for FarCallCounter {
        fn before_instruction<S: crate::StateInterface>(&mut self, _: &mut S) {
            self.0 += 1;
        }
    }

    #[test]
    fn test_tracer() {
        let mut tracer = FarCallCounter(0);

        tracer
            .opcode::<opcodes::Add>()
            .before_instruction(&mut DummyState);
        assert_eq!(tracer.0, 0);

        tracer
            .opcode::<opcodes::FarCall>()
            .before_instruction(&mut DummyState);
        assert_eq!(tracer.0, 1);
    }

    #[test]
    fn test_aggregate_tracer() {
        let mut tracer = (FarCallCounter(0), (FarCallCounter(0), FarCallCounter(0)));

        tracer
            .opcode::<opcodes::Sub>()
            .before_instruction(&mut DummyState);
        assert_eq!(tracer.0 .0, 0);
        assert_eq!(tracer.1 .0 .0, 0);
        assert_eq!(tracer.1 .1 .0, 0);

        tracer
            .opcode::<opcodes::FarCall>()
            .before_instruction(&mut DummyState);
        assert_eq!(tracer.0 .0, 1);
        assert_eq!(tracer.1 .0 .0, 1);
        assert_eq!(tracer.1 .1 .0, 1);
    }
}
