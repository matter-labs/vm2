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
    pub struct FarCall<const M: u8>;
    pub struct Ret<const M: u8>;
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

#[derive(PartialEq, Eq)]
#[repr(u8)]
pub enum CallingMode {
    Normal = 0,
    Delegate,
    Mimic,
}

impl CallingMode {
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => CallingMode::Normal,
            1 => CallingMode::Delegate,
            2 => CallingMode::Mimic,
            _ => unreachable!(),
        }
    }
}

#[repr(u8)]
#[derive(PartialEq, Eq)]
pub enum ReturnType {
    Normal = 0,
    Revert,
    Panic,
}

impl ReturnType {
    pub fn is_failure(&self) -> bool {
        *self != ReturnType::Normal
    }

    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => ReturnType::Normal,
            1 => ReturnType::Revert,
            2 => ReturnType::Panic,
            _ => unreachable!(),
        }
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

impl<const M: u8> OpcodeType for opcodes::FarCall<M> {
    const VALUE: Opcode = Opcode::FarCall(CallingMode::from_u8(M));
}

impl<const M: u8> OpcodeType for opcodes::Ret<M> {
    const VALUE: Opcode = Opcode::Ret(ReturnType::from_u8(M));
}

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
///             Opcode::FarCall(_) => self.0 += 1,
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
    use super::{CallingMode, OpcodeType};
    use crate::{opcodes, DummyState, Tracer};

    struct FarCallCounter(usize);

    impl Tracer for FarCallCounter {
        fn before_instruction<OP: OpcodeType, S: crate::StateInterface>(&mut self, _: &mut S) {
            match OP::VALUE {
                super::Opcode::FarCall(CallingMode::Normal) => self.0 += 1,
                _ => {}
            }
        }
    }

    #[test]
    fn test_tracer() {
        let mut tracer = FarCallCounter(0);

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0, 0);

        tracer.before_instruction::<opcodes::FarCall<{ CallingMode::Normal as u8 }>, _>(
            &mut DummyState,
        );
        assert_eq!(tracer.0, 1);

        tracer.before_instruction::<opcodes::FarCall<{ CallingMode::Delegate as u8 }>, _>(
            &mut DummyState,
        );
        assert_eq!(tracer.0, 1);
    }

    #[test]
    fn test_aggregate_tracer() {
        let mut tracer = (FarCallCounter(0), (FarCallCounter(0), FarCallCounter(0)));

        tracer.before_instruction::<opcodes::Nop, _>(&mut DummyState);
        assert_eq!(tracer.0 .0, 0);
        assert_eq!(tracer.1 .0 .0, 0);
        assert_eq!(tracer.1 .1 .0, 0);

        tracer.before_instruction::<opcodes::FarCall<{ CallingMode::Normal as u8 }>, _>(
            &mut DummyState,
        );
        assert_eq!(tracer.0 .0, 1);
        assert_eq!(tracer.1 .0 .0, 1);
        assert_eq!(tracer.1 .1 .0, 1);
    }
}
