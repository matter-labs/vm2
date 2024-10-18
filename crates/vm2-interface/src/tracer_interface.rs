use crate::GlobalStateInterface;

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
        #[doc = concat!("`", stringify!($x), "` opcode.")]
        #[derive(Debug)]
        pub struct $x;
    };
}

/// EraVM opcodes.
pub mod opcodes {
    use std::marker::PhantomData;

    use super::{CallingMode, ReturnType};

    forall_simple_opcodes!(pub_struct);

    /// `FarCall` group of opcodes distinguished by the calling mode (normal, delegate, or mimic).
    #[derive(Debug)]
    pub struct FarCall<M: TypeLevelCallingMode>(PhantomData<M>);

    /// `Ret` group of opcodes distinguished by the return type (normal, panic, or revert).
    #[derive(Debug)]
    pub struct Ret<T: TypeLevelReturnType>(PhantomData<T>);

    /// Normal [`Ret`]urn mode / [`FarCall`] mode.
    #[derive(Debug)]
    pub struct Normal;

    /// Delegate [`FarCall`] mode.
    #[derive(Debug)]
    pub struct Delegate;

    /// Mimic [`FarCall`] mode.
    #[derive(Debug)]
    pub struct Mimic;

    /// Revert [`Ret`]urn mode.
    #[derive(Debug)]
    pub struct Revert;

    /// Panic [`Ret`]urn mode.
    #[derive(Debug)]
    pub struct Panic;

    /// Calling mode for the [`FarCall`] opcodes.
    pub trait TypeLevelCallingMode {
        /// Constant corresponding to this mode allowing to easily `match` it.
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

    /// Return type for the [`Ret`] opcodes.
    pub trait TypeLevelReturnType {
        /// Constant corresponding to this return type allowing to easily `match` it.
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

/// All supported EraVM opcodes in a single enumeration.
#[allow(missing_docs)]
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

/// All supported calling modes for [`FarCall`](opcodes::FarCall) opcode.
#[derive(PartialEq, Eq, Debug, Copy, Clone, Hash)]
pub enum CallingMode {
    /// Normal calling mode.
    Normal,
    /// Delegate calling mode (similar to `delegatecall` in EVM).
    Delegate,
    /// Mimic calling mode (can only be used by system contracts; allows to emulate `eth_call` semantics while retaining the bootloader).
    Mimic,
}

/// All supported return types for the [`Ret`](opcodes::Ret) opcode.
#[derive(PartialEq, Eq, Debug, Copy, Clone, Hash)]
pub enum ReturnType {
    /// Normal return.
    Normal,
    /// Revert (e.g., a result of a Solidity `revert`).
    Revert,
    /// Panic, i.e. a non-revert abnormal control flow termination (e.g., out of gas).
    Panic,
}

impl ReturnType {
    /// Checks if this return type is [normal](Self::Normal).
    pub fn is_failure(&self) -> bool {
        *self != ReturnType::Normal
    }
}

/// Trait mapping opcodes as types to the corresponding variants of the [`Opcode`] enum.
pub trait OpcodeType {
    /// `Opcode` variant corresponding to this opcode type.
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

/// EraVM instruction tracer.
///
/// [`Self::before_instruction()`] is called just before the actual instruction is executed.
/// If the instruction is skipped, `before_instruction` will be called with [`Nop`](opcodes::Nop).
/// [`Self::after_instruction()`] is called once the instruction is executed and the program
/// counter has advanced.
///
/// # Examples
///
/// Here `FarCallCounter` counts the number of far calls.
///
/// ```
/// # use zksync_vm2_interface::{Tracer, GlobalStateInterface, OpcodeType, Opcode};
/// struct FarCallCounter(usize);
///
/// impl Tracer for FarCallCounter {
///     fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
///         match OP::VALUE {
///             Opcode::FarCall(_) => self.0 += 1,
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait Tracer {
    /// Executes logic before an instruction handler.
    ///
    /// The default implementation does nothing.
    fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        let _ = state;
    }
    /// Executes logic after an instruction handler.
    ///
    /// The default implementation does nothing.
    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        let _ = state;
    }

    /// Provides cycle statistics for "complex" instructions from the prover perspective (mostly precompile calls).
    ///
    /// The default implementation does nothing.
    fn on_extra_prover_cycles(&mut self, _stats: CycleStats) {}
}

/// Cycle statistics emitted by the VM and supplied to [`Tracer::on_extra_prover_cycles()`].
#[derive(Debug, Clone, Copy)]
pub enum CycleStats {
    /// Call to the `keccak256` precompile with the specified number of hash cycles.
    Keccak256(u32),
    /// Call to the `sha256` precompile with the specified number of hash cycles.
    Sha256(u32),
    /// Call to the `ecrecover` precompile with the specified number of hash cycles.
    EcRecover(u32),
    /// Call to the `secp256r1_verify` precompile with the specified number of hash cycles.
    Secp256r1Verify(u32),
    /// Decommitting an opcode.
    Decommit(u32),
    /// Reading a slot from the VM storage.
    StorageRead,
    /// Writing a slot to the VM storage.
    StorageWrite,
}

/// No-op tracer implementation.
impl Tracer for () {}

// Multiple tracers can be combined by building a linked list out of tuples.
impl<A: Tracer, B: Tracer> Tracer for (A, B) {
    fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        self.0.before_instruction::<OP, S>(state);
        self.1.before_instruction::<OP, S>(state);
    }

    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        self.0.after_instruction::<OP, S>(state);
        self.1.after_instruction::<OP, S>(state);
    }

    fn on_extra_prover_cycles(&mut self, stats: CycleStats) {
        self.0.on_extra_prover_cycles(stats);
        self.1.on_extra_prover_cycles(stats);
    }
}

#[cfg(test)]
mod tests {
    use super::{CallingMode, OpcodeType};
    use crate::{opcodes, tests::DummyState, GlobalStateInterface, Tracer};

    struct FarCallCounter(usize);

    impl Tracer for FarCallCounter {
        fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, _: &mut S) {
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
