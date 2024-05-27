use zk_evm::{
    aux_structures::{MemoryPage, PubdataCost},
    vm_state::{execution_stack::CallStackEntry, Callstack, PrimitiveValue, VmLocalState},
};
use zkevm_opcode_defs::decoding::EncodingModeProduction;

use crate::callframe::Callframe;

pub(crate) fn vm2_state_to_zk_evm_state(
    state: &crate::State,
) -> VmLocalState<8, EncodingModeProduction> {
    VmLocalState {
        // To ensure that this field is not read, we make previous_super_pc != super_pc
        previous_code_word: 0.into(),
        previous_code_memory_page: MemoryPage(0),
        registers: state
            .registers
            .into_iter()
            .enumerate()
            .skip(1)
            .map(|(i, value)| PrimitiveValue {
                value,
                is_pointer: state.register_pointer_flags & (1 << i) != 0,
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        flags: (&state.flags).into(),
        timestamp: 0,
        monotonic_cycle_counter: 0,
        spent_pubdata_counter: 0, // This field is unused
        memory_page_counter: 3000,
        absolute_execution_step: 0,
        tx_number_in_block: state.transaction_number,
        pending_exception: false,
        previous_super_pc: 13, // Current pc is zero so anything else is fine
        context_u128_register: state.context_u128,
        callstack: Callstack {
            current: (&state.current_frame).into(),
            inner: state
                .previous_frames
                .iter()
                .map(|(_, frame)| frame.into())
                .collect(),
        },
        pubdata_revert_counter: PubdataCost(0),
    }
}

impl From<&Callframe> for CallStackEntry {
    fn from(frame: &Callframe) -> Self {
        CallStackEntry {
            this_address: frame.address,
            msg_sender: frame.caller,
            code_address: frame.code_address,
            base_memory_page: MemoryPage(frame.heap.to_u32()),
            code_page: MemoryPage(0), // TODO
            sp: frame.sp,
            pc: 0,
            exception_handler_location: frame.exception_handler,
            ergs_remaining: frame.gas,
            this_shard_id: 0,
            caller_shard_id: 0,
            code_shard_id: 0,
            is_static: frame.is_static,
            is_local_frame: false, // TODO this is for making near calls
            context_u128_value: frame.context_u128,
            heap_bound: frame.heap_size,
            aux_heap_bound: frame.aux_heap_size,
            total_pubdata_spent: PubdataCost(0),
            stipend: frame.stipend,
        }
    }
}

pub(crate) fn zk_evm_state_equal(
    vm1: &VmLocalState<8, EncodingModeProduction>,
    vm2: &VmLocalState<8, EncodingModeProduction>,
) -> bool {
    vm1.registers == vm2.registers
        && vm1.flags == vm2.flags
        && vm1.tx_number_in_block == vm2.tx_number_in_block
        && vm1.context_u128_register == vm2.context_u128_register
        && zk_evm_callframe_equal(&vm1.callstack.current, &vm2.callstack.current)
        && vm1
            .callstack
            .inner
            .iter()
            .zip(vm2.callstack.inner.iter())
            .all(|(frame1, frame2)| zk_evm_callframe_equal(frame1, frame2))
}

fn zk_evm_callframe_equal(frame1: &CallStackEntry, frame2: &CallStackEntry) -> bool {
    frame1.this_address == frame2.this_address
        && frame1.msg_sender == frame2.msg_sender
        && frame1.code_address == frame2.code_address
        && frame1.sp == frame2.sp
        && frame1.exception_handler_location == frame2.exception_handler_location
        && frame1.ergs_remaining == frame2.ergs_remaining
        && frame1.is_static == frame2.is_static
        && frame1.is_local_frame == frame2.is_local_frame
        && frame1.context_u128_value == frame2.context_u128_value
        && frame1.heap_bound == frame2.heap_bound
        && frame1.aux_heap_bound == frame2.aux_heap_bound
        && frame1.stipend == frame2.stipend
}
