use crate::StateInterface;

macro_rules! forall_opcodes {
    ($m:ident) => {
        $m!(Nop);
        $m!(Add);
        $m!(Sub);
        $m!(And);
        $m!(Or);
        $m!(Xor);
        $m!(ShiftLeft);
        $m!(ShiftRight);
        $m!(RotateLeft);
        $m!(RotateRight);
        $m!(Mul);
        $m!(Div);
        $m!(NearCall);
        $m!(FarCall);
        $m!(Ret);
        $m!(Jump);
        $m!(Event);
        $m!(L2ToL1Message);
        $m!(Decommit);
        $m!(This);
        $m!(Caller);
        $m!(CodeAddress);
        $m!(ErgsLeft);
        $m!(SP);
        $m!(ContextMeta);
        $m!(ContextU128);
        $m!(SetContextU128);
        $m!(IncrementTxNumber);
        $m!(AuxMutating0);
        $m!(PrecompileCall);
        $m!(HeapRead);
        $m!(HeapWrite);
        $m!(AuxHeapRead);
        $m!(AuxHeapWrite);
        $m!(PointerRead);
        $m!(PointerAdd);
        $m!(PointerSub);
        $m!(PointerPack);
        $m!(PointerShrink);
        $m!(StorageRead);
        $m!(StorageWrite);
        $m!(TransientStorageRead);
        $m!(TransientStorageWrite);
    };
}

macro_rules! pub_struct {
    ($x:ident) => {
        pub struct $x;
    };
}

pub mod opcodes {
    forall_opcodes!(pub_struct);
}

#[derive(PartialEq, Eq)]
pub enum Opcode {
    Nop,
    Add,
    Sub,
    And,
    Or,
    Xor,
    ShiftLeft,
    ShiftRight,
    RotateLeft,
    RotateRight,
    Mul,
    Div,
    NearCall,
    FarCall,
    Ret,
    Jump,
    Event,
    L2ToL1Message,
    Decommit,
    This,
    Caller,
    CodeAddress,
    ErgsLeft,
    SP,
    ContextMeta,
    ContextU128,
    SetContextU128,
    IncrementTxNumber,
    AuxMutating0,
    PrecompileCall,
    HeapRead,
    HeapWrite,
    AuxHeapRead,
    AuxHeapWrite,
    PointerRead,
    PointerAdd,
    PointerSub,
    PointerPack,
    PointerShrink,
    StorageRead,
    StorageWrite,
    TransientStorageRead,
    TransientStorageWrite,
}

pub trait OpcodeType {
    const VALUE: Opcode;
}

macro_rules! impl_opcode {
    ($x:ident) => {
        impl OpcodeType for opcodes::$x {
            const VALUE: Opcode = Opcode::$x;
        }
    };
}

forall_opcodes!(impl_opcode);

pub trait Tracer {
    fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
    fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
}

/// For example, here `FarCallCounter` counts the number of far calls.
/// ```
/// use eravm_stable_interface::{Tracer, StateInterface, OpcodeType, Opcode};
/// struct FarCallCounter(usize);
/// impl Tracer for FarCallCounter {
///     fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
///         match OP::VALUE {
///             Opcode::FarCall => self.0 += 1,
///             _ => {}
///         }
///     }
/// }
/// ```

impl Tracer for () {}

// Multiple tracers can be combined by building a linked list out of tuples.
impl<A: Tracer, B: Tracer> Tracer for (A, B) {
    fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
        self.0.before_instruction::<OP, S>(state);
        self.1.before_instruction::<OP, S>(state);
    }

    fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
        self.0.after_instruction::<OP, S>(state);
        self.1.after_instruction::<OP, S>(state);
    }
}

#[cfg(test)]
mod tests {
    use crate::{opcodes, DummyState, Tracer};

    use super::OpcodeType;

    struct FarCallCounter(usize);

    impl Tracer for FarCallCounter {
        fn before_instruction<OP: OpcodeType, S: crate::StateInterface>(&mut self, _: &mut S) {
            if OP::VALUE == super::Opcode::FarCall {
                self.0 += 1;
            }
        }
    }

    #[test]
    fn test_tracer() {
        let mut tracer = FarCallCounter(0);

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0, 0);

        tracer.before_instruction::<opcodes::FarCall, _>(&mut DummyState);
        assert_eq!(tracer.0, 1);
    }

    #[test]
    fn test_aggregate_tracer() {
        let mut tracer = (FarCallCounter(0), (FarCallCounter(0), FarCallCounter(0)));

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0 .0, 0);
        assert_eq!(tracer.1 .0 .0, 0);
        assert_eq!(tracer.1 .1 .0, 0);

        tracer.before_instruction::<opcodes::FarCall, _>(&mut DummyState);
        assert_eq!(tracer.0 .0, 1);
        assert_eq!(tracer.1 .0 .0, 1);
        assert_eq!(tracer.1 .1 .0, 1);
    }
}
