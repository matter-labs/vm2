use crate::StateInterface;

#[macro_export]
macro_rules! forall_opcodes {
    ($m: ident) => {
        $m!(Nop, before_nop, after_nop);
        $m!(Add, before_add, after_add);
        $m!(Sub, before_sub, after_sub);
        $m!(And, before_and, after_and);
        $m!(Or, before_or, after_or);
        $m!(Xor, before_xor, after_xor);
        $m!(ShiftLeft, before_shift_left, after_shift_left);
        $m!(ShiftRight, before_shift_right, after_shift_right);
        $m!(RotateLeft, before_rotate_left, after_rotate_left);
        $m!(RotateRight, before_rotate_right, after_rotate_right);
        $m!(Mul, before_mul, after_mul);
        $m!(Div, before_div, after_div);
        $m!(NearCall, before_near_call, after_near_call);
        $m!(FarCall, before_far_call, after_far_call);
        $m!(Ret, before_ret, after_ret);
        $m!(Jump, before_jump, after_jump);
        $m!(Event, before_event, after_event);
        $m!(L2ToL1Message, before_l1_message, after_l1_message);
        $m!(Decommit, before_decommit, after_decommit);
        $m!(This, before_this, after_this);
        $m!(Caller, before_caller, after_caller);
        $m!(CodeAddress, before_code_address, after_code_address);
        $m!(ErgsLeft, before_ergs_left, after_ergs_left);
        $m!(U128, before_u128, after_u128);
        $m!(SP, before_sp, after_sp);
        $m!(ContextMeta, before_context_meta, after_context_meta);
        $m!(
            SetContextU128,
            before_set_context_u128,
            after_set_context_u128
        );
        $m!(
            IncrementTxNumber,
            before_increment_tx_number,
            after_increment_tx_number
        );
        $m!(AuxMutating0, before_aux_mutating0, after_aux_mutating0);
        $m!(
            PrecompileCall,
            before_precompile_call,
            after_precompile_call
        );
        $m!(HeapRead, before_heap_read, after_heap_read);
        $m!(HeapWrite, before_heap_write, after_heap_write);
        $m!(AuxHeapRead, before_aux_heap_read, after_aux_heap_read);
        $m!(AuxHeapWrite, before_aux_heap_write, after_aux_heap_write);
        $m!(PointerRead, before_pointer_read, after_pointer_read);
        $m!(PointerAdd, before_pointer_add, after_pointer_add);
        $m!(PointerSub, before_pointer_sub, after_pointer_sub);
        $m!(PointerPack, before_pointer_pack, after_pointer_pack);
        $m!(PointerShrink, before_pointer_shrink, after_pointer_shrink);
        $m!(StorageRead, before_storage_read, after_storage_read);
        $m!(StorageWrite, before_storage_write, after_storage_write);
        $m!(
            TransientStorageRead,
            before_transient_storage_read,
            after_transient_storage_read
        );
        $m!(
            TransientStorageWrite,
            before_transient_storage_write,
            after_transient_storage_write
        );
        $m!(
            StaticMemoryRead,
            before_static_memory_read,
            after_static_memory_read
        );
        $m!(
            StaticMemoryWrite,
            before_static_memory_write,
            after_static_memory_write
        );
    };
}

macro_rules! into_default_method_implementations {
    ($op:ident, $before_method:ident, $after_method:ident) => {
        #[inline(always)]
        fn $before_method<S: StateInterface>(&mut self, _state: &mut S) {}
        #[inline(always)]
        fn $after_method<S: StateInterface>(&mut self, _state: &mut S) {}
    };
}

/// For example, here `FarCallCounter` counts the number of far calls.
/// ```
/// use eravm_stable_interface::{Tracer, opcodes, StateInterface};
/// struct FarCallCounter(usize);
/// impl Tracer for FarCallCounter {
///     fn before_far_call<S: StateInterface>(&mut self, state: &mut S) {
///         self.0 += 1;
///     }
/// }
/// ```
pub trait Tracer {
    forall_opcodes! {
        into_default_method_implementations
    }
}

impl Tracer for () {}

macro_rules! dispatch_to_tracer_tuple {
    ($op:ident, $before_method:ident, $after_method:ident) => {
        fn $before_method<S: crate::StateInterface>(&mut self, state: &mut S) {
            self.0.$before_method(state);
            self.1.$before_method(state);
        }
        fn $after_method<S: crate::StateInterface>(&mut self, state: &mut S) {
            self.0.$after_method(state);
            self.1.$after_method(state);
        }
    };
}

// Multiple tracers can be combined by building a linked list out of tuples.
impl<A: Tracer, B: Tracer> Tracer for (A, B) {
    forall_opcodes!(dispatch_to_tracer_tuple);
}

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
    pub struct AuxHeapRead;
    pub struct AuxHeapWrite;
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
    use crate::{DummyState, Tracer};

    struct FarCallCounter(usize);

    impl Tracer for FarCallCounter {
        fn before_far_call<S: crate::StateInterface>(&mut self, _: &mut S) {
            self.0 += 1;
        }
    }

    #[test]
    fn test_tracer() {
        let mut tracer = FarCallCounter(0);

        tracer.before_nop(&mut DummyState);
        assert_eq!(tracer.0, 0);

        tracer.before_far_call(&mut DummyState);
        assert_eq!(tracer.0, 1);
    }

    #[test]
    fn test_aggregate_tracer() {
        let mut tracer = (FarCallCounter(0), (FarCallCounter(0), FarCallCounter(0)));

        tracer.before_nop(&mut DummyState);
        assert_eq!(tracer.0 .0, 0);
        assert_eq!(tracer.1 .0 .0, 0);
        assert_eq!(tracer.1 .1 .0, 0);

        tracer.before_far_call(&mut DummyState);
        assert_eq!(tracer.0 .0, 1);
        assert_eq!(tracer.1 .0 .0, 1);
        assert_eq!(tracer.1 .1 .0, 1);
    }
}
