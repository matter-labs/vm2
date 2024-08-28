use crate::StateInterface;

macro_rules! forall_simple_opcodes {
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
    forall_simple_opcodes!(pub_struct);
    pub struct FarCall<M: TypeLevelCallingMode>(M);
    pub struct Ret<T: TypeLevelReturnType>(T);

    pub struct Normal;
    pub struct Delegate;
    pub struct Mimic;
    pub struct Revert;
    pub struct Panic;

    use super::{CallingMode, ReturnType};

    pub trait TypeLevelCallingMode {
        const VALUE: CallingMode;
    }

    impl TypeLevelCallingMode for Normal {
        const VALUE: CallingMode = CallingMode::Normal;
    }
    impl TypeLevelCallingMode for Delegate {
        const VALUE: CallingMode = CallingMode::Delegate;
    }
    impl TypeLevelCallingMode for Mimic {
        const VALUE: CallingMode = CallingMode::Mimic;
    }

    pub trait TypeLevelReturnType {
        const VALUE: ReturnType;
    }

    impl TypeLevelReturnType for Normal {
        const VALUE: ReturnType = ReturnType::Normal;
    }
    impl TypeLevelReturnType for Revert {
        const VALUE: ReturnType = ReturnType::Revert;
    }
    impl TypeLevelReturnType for Panic {
        const VALUE: ReturnType = ReturnType::Panic;
    }
}

#[derive(PartialEq, Eq, Debug, Copy, Clone, Hash)]
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
    FarCall(CallingMode),
    Ret(ReturnType),
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

#[derive(PartialEq, Eq, Debug, Copy, Clone, Hash)]
pub enum CallingMode {
    Normal,
    Delegate,
    Mimic,
}

#[derive(PartialEq, Eq, Debug, Copy, Clone, Hash)]
pub enum ReturnType {
    Normal,
    Revert,
    Panic,
}

impl ReturnType {
    pub fn is_failure(&self) -> bool {
        *self != ReturnType::Normal
    }
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

forall_simple_opcodes!(impl_opcode);

impl<M: opcodes::TypeLevelCallingMode> OpcodeType for opcodes::FarCall<M> {
    const VALUE: Opcode = Opcode::FarCall(M::VALUE);
}

impl<T: opcodes::TypeLevelReturnType> OpcodeType for opcodes::Ret<T> {
    const VALUE: Opcode = Opcode::Ret(T::VALUE);
}

/// Implement this for a type that holds the state of your tracer.
///
/// [Tracer::before_instruction] is called just before the actual instruction is executed.
/// If the instruction is skipped, `before_instruction` will be called with [Nop](opcodes::Nop).
/// [Tracer::after_instruction] is called once the instruction is executed and the program
/// counter has advanced.
///
/// # Examples
/// Here `FarCallCounter` counts the number of far calls.
/// ```
/// use eravm_stable_interface::{Tracer, StateInterface, OpcodeType, Opcode};
/// struct FarCallCounter(usize);
/// impl Tracer for FarCallCounter {
///     fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
///         match OP::VALUE {
///             Opcode::FarCall(_) => self.0 += 1,
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait Tracer {
    fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
    fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
}

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
    use super::{CallingMode, OpcodeType};
    use crate::{opcodes, DummyState, Tracer};

    struct FarCallCounter(usize);

    impl Tracer for FarCallCounter {
        fn before_instruction<OP: OpcodeType, S: crate::StateInterface>(&mut self, _: &mut S) {
            if let super::Opcode::FarCall(CallingMode::Normal) = OP::VALUE {
                self.0 += 1;
            }
        }
    }

    #[test]
    fn test_tracer() {
        let mut tracer = FarCallCounter(0);

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0, 0);

        tracer.before_instruction::<opcodes::FarCall<opcodes::Normal>, _>(&mut DummyState);
        assert_eq!(tracer.0, 1);

        tracer.before_instruction::<opcodes::FarCall<opcodes::Mimic>, _>(&mut DummyState);
        assert_eq!(tracer.0, 1);
    }

    #[test]
    fn test_aggregate_tracer() {
        let mut tracer = (FarCallCounter(0), (FarCallCounter(0), FarCallCounter(0)));

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0 .0, 0);
        assert_eq!(tracer.1 .0 .0, 0);
        assert_eq!(tracer.1 .1 .0, 0);

        tracer.before_instruction::<opcodes::FarCall<opcodes::Normal>, _>(&mut DummyState);
        assert_eq!(tracer.0 .0, 1);
        assert_eq!(tracer.1 .0 .0, 1);
        assert_eq!(tracer.1 .1 .0, 1);
    }
}
