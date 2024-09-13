use std::iter;

use primitive_types::U256;
use zk_evm::{
    aux_structures::{MemoryPage, PubdataCost},
    vm_state::{execution_stack::CallStackEntry, Callstack, PrimitiveValue, VmLocalState},
};
use zkevm_opcode_defs::decoding::EncodingModeProduction;
use zksync_vm2_interface::Tracer;

use crate::{
    callframe::{Callframe, NearCallFrame},
    state::State,
    Instruction, World,
};

pub(crate) fn vm2_state_to_zk_evm_state<T: Tracer, W: World<T>>(
    state: &State<T, W>,
    panic: &Instruction<T, W>,
) -> VmLocalState<8, EncodingModeProduction> {
    // zk_evm requires an unused bottom frame
    let mut callframes: Vec<_> = iter::once(CallStackEntry::empty_context())
        .chain(
            state
                .previous_frames
                .iter()
                .cloned()
                .chain(iter::once(state.current_frame.clone()))
                .flat_map(vm2_frame_to_zk_evm_frames),
        )
        .collect();

    VmLocalState {
        previous_code_word: U256([0, 0, 0, state.current_frame.raw_first_instruction()]),
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
        pending_exception: state.current_frame.pc == panic,
        previous_super_pc: 0, // Same as current pc so the instruction is read from previous_code_word
        context_u128_register: state.context_u128,
        callstack: Callstack {
            current: callframes.pop().unwrap(),
            // zk_evm requires an unused bottom frame
            inner: callframes,
        },
        pubdata_revert_counter: PubdataCost(0),
    }
}

fn vm2_frame_to_zk_evm_frames<T, W>(
    frame: Callframe<T, W>,
) -> impl Iterator<Item = CallStackEntry> {
    let far_frame = CallStackEntry {
        this_address: frame.address,
        msg_sender: frame.caller,
        code_address: frame.code_address,
        base_memory_page: MemoryPage(frame.heap.as_u32() - 2),
        code_page: MemoryPage(0), // TODO
        sp: frame.sp,
        pc: 0,
        exception_handler_location: frame.exception_handler,
        ergs_remaining: frame.gas,
        this_shard_id: 0,
        caller_shard_id: 0,
        code_shard_id: 0,
        is_static: frame.is_static,
        is_local_frame: false,
        context_u128_value: frame.context_u128,
        heap_bound: frame.heap_size,
        aux_heap_bound: frame.aux_heap_size,
        total_pubdata_spent: PubdataCost(0),
        stipend: frame.stipend,
    };

    let mut result = vec![far_frame];
    for NearCallFrame {
        exception_handler,
        previous_frame_sp,
        previous_frame_gas,
        previous_frame_pc,
        ..
    } in frame.near_calls
    {
        let last = result.last_mut().unwrap();
        last.pc = previous_frame_pc;
        last.sp = previous_frame_sp;
        last.ergs_remaining = previous_frame_gas;

        result.push(CallStackEntry {
            is_local_frame: true,
            exception_handler_location: exception_handler,
            ..far_frame
        });
    }

    result.into_iter()
}
