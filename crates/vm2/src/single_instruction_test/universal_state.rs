use std::collections::BTreeMap;

use primitive_types::{H160, U256};
use zk_evm::{
    aux_structures::LogQuery,
    vm_state::{CallStackEntry, VmLocalState, VmState},
};
use zkevm_opcode_defs::{
    decoding::EncodingModeProduction,
    system_params::{EVENT_AUX_BYTE, L1_MESSAGE_AUX_BYTE},
    ADDRESS_EVENT_WRITER,
};
use zksync_vm2_interface::{Event, L2ToL1Log, Tracer};

use super::{
    into_zk_evm::{MockDecommitter, MockEventSink, MockMemory, MockWorldWrapper, NoOracle},
    state_to_zk_evm::vm2_state_to_zk_evm_state,
};
use crate::{VirtualMachine, World};

#[derive(PartialEq, Debug)]
pub struct UniversalVmState {
    registers: [(U256, bool); 15],
    flags: [bool; 3],
    transaction_number: u16,
    context_u128: u128,
    frames: Vec<UniversalVmFrame>,
    events: Vec<UniversalEvent>,
    l2_to_l1_logs: Vec<UniversalEvent>,
    transient_storage_logs: Vec<UniversalTransientLog>,
    will_panic: bool,
}

#[derive(PartialEq, Debug)]
pub(crate) struct UniversalVmFrame {
    address: H160,
    caller: H160,
    code_address: H160,
    sp: u16,
    exception_handler: u16,
    gas: u32,
    is_static: bool,
    is_near_call: bool,
    context_u128: u128,
    heap_bound: u32,
    aux_heap_bound: u32,
    stipend: u32,
}

#[derive(PartialEq, Debug)]
struct UniversalEvent {
    address: H160,
    key: U256,
    value: U256,
    is_first: bool,
    shard_id: u8,
    tx_number: u16,
}

#[derive(PartialEq, Debug)]
struct UniversalTransientLog {
    address: H160,
    key: U256,
    read_value: U256,
    written_value: U256,
    rw_flag: bool,
    rollback: bool,
    is_service: bool,
    shard_id: u8,
    tx_number: u16,
}

impl
    From<
        VmState<
            MockWorldWrapper,
            MockMemory,
            MockEventSink,
            NoOracle,
            MockDecommitter,
            NoOracle,
            8,
            EncodingModeProduction,
        >,
    > for UniversalVmState
{
    fn from(
        vm: VmState<
            MockWorldWrapper,
            MockMemory,
            MockEventSink,
            NoOracle,
            MockDecommitter,
            NoOracle,
            8,
            EncodingModeProduction,
        >,
    ) -> Self {
        let mut state = zk_evm_state_to_universal(&vm.local_state);
        let (events, l2_to_l1_logs) = zk_evm_events_to_universal(&vm.event_sink);
        state.events = events;
        state.l2_to_l1_logs = l2_to_l1_logs;
        state.transient_storage_logs = vm
            .storage
            .transient_logs()
            .iter()
            .map(zk_evm_transient_log_to_universal)
            .collect();
        state
    }
}

pub fn vm2_to_universal<T: Tracer, W: World<T>>(vm: &VirtualMachine<T, W>) -> UniversalVmState {
    let local_state = vm2_state_to_zk_evm_state(&vm.state);
    let mut state = zk_evm_state_to_universal(&local_state);
    state.events = vm
        .world_diff
        .events()
        .iter()
        .map(vm2_event_to_universal)
        .collect();
    state.l2_to_l1_logs = vm
        .world_diff
        .l2_to_l1_logs()
        .iter()
        .map(vm2_l2_to_l1_log_to_universal)
        .collect();
    state.transient_storage_logs = Vec::new();
    state
}

fn zk_evm_state_to_universal(vm: &VmLocalState<8, EncodingModeProduction>) -> UniversalVmState {
    let mut current_frame = zk_evm_frame_to_universal(&vm.callstack.current);
    // Most of the current frame doesn't matter if we panic, as it is just thrown away
    // but only sp has proved problematic so far
    if vm.pending_exception {
        current_frame.sp = 0;
    }

    let mut frames = vm
        .callstack
        .inner
        .iter()
        .skip(1) // zk_evm requires an unused bottom frame
        .map(zk_evm_frame_to_universal)
        .chain(std::iter::once(current_frame))
        .collect::<Vec<_>>();

    // In zk_evm, heap bounds grown in a near call are only propagated on return, so we need to work around that
    let mut previous_heap_bounds = None;
    for frame in frames.iter_mut().rev() {
        if let Some((heap_bound, aux_heap_bound)) = previous_heap_bounds {
            frame.heap_bound = heap_bound;
            frame.aux_heap_bound = aux_heap_bound;
        }
        if frame.is_near_call {
            previous_heap_bounds = Some((frame.heap_bound, frame.aux_heap_bound));
        } else {
            previous_heap_bounds = None;
        }
    }

    UniversalVmState {
        registers: vm
            .registers
            .iter()
            .map(|value| (value.value, value.is_pointer))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        flags: [
            vm.flags.overflow_or_less_than_flag,
            vm.flags.equality_flag,
            vm.flags.greater_than_flag,
        ],
        transaction_number: vm.tx_number_in_block,
        context_u128: vm.context_u128_register,
        frames,
        events: Vec::new(),
        l2_to_l1_logs: Vec::new(),
        transient_storage_logs: Vec::new(),
        will_panic: vm.pending_exception,
    }
}

fn zk_evm_frame_to_universal(frame: &CallStackEntry) -> UniversalVmFrame {
    UniversalVmFrame {
        address: frame.this_address,
        caller: frame.msg_sender,
        code_address: frame.code_address,
        sp: frame.sp,
        exception_handler: frame.exception_handler_location,
        gas: frame.ergs_remaining,
        is_static: frame.is_static,
        is_near_call: frame.is_local_frame,
        context_u128: frame.context_u128_value,
        heap_bound: frame.heap_bound,
        aux_heap_bound: frame.aux_heap_bound,
        stipend: frame.stipend,
    }
}

fn vm2_event_to_universal(event: &Event) -> UniversalEvent {
    UniversalEvent {
        address: H160::from_low_u64_be(ADDRESS_EVENT_WRITER.into()),
        key: event.key,
        value: event.value,
        is_first: event.is_first,
        shard_id: event.shard_id,
        tx_number: event.tx_number,
    }
}

fn vm2_l2_to_l1_log_to_universal(log: &L2ToL1Log) -> UniversalEvent {
    UniversalEvent {
        address: log.address,
        key: log.key,
        value: log.value,
        is_first: log.is_service,
        shard_id: log.shard_id,
        tx_number: log.tx_number,
    }
}

fn zk_evm_events_to_universal(
    event_sink: &MockEventSink,
) -> (Vec<UniversalEvent>, Vec<UniversalEvent>) {
    let mut flattened = BTreeMap::new();
    for query in event_sink
        .frames_stack
        .iter()
        .flat_map(|frame| frame.forward.iter())
    {
        if flattened.contains_key(&query.timestamp.0) {
            if query.rollback {
                flattened.remove(&query.timestamp.0);
            }
        } else if !query.rollback {
            flattened.insert(query.timestamp.0, query);
        }
    }

    let mut events = Vec::new();
    let mut l2_to_l1_logs = Vec::new();

    for query in flattened.into_values() {
        if let Some(event) = zk_evm_log_query_to_universal(query) {
            match query.aux_byte {
                EVENT_AUX_BYTE => events.push(event),
                L1_MESSAGE_AUX_BYTE => l2_to_l1_logs.push(event),
                _ => {}
            }
        }
    }

    (events, l2_to_l1_logs)
}

fn zk_evm_log_query_to_universal(query: &LogQuery) -> Option<UniversalEvent> {
    if query.rollback {
        return None;
    }
    if query.aux_byte != EVENT_AUX_BYTE && query.aux_byte != L1_MESSAGE_AUX_BYTE {
        return None;
    }

    Some(UniversalEvent {
        address: query.address,
        key: query.key,
        value: query.written_value,
        is_first: query.is_service,
        shard_id: query.shard_id,
        tx_number: query.tx_number_in_block,
    })
}

fn zk_evm_transient_log_to_universal(query: &LogQuery) -> UniversalTransientLog {
    UniversalTransientLog {
        address: query.address,
        key: query.key,
        read_value: query.read_value,
        written_value: query.written_value,
        rw_flag: query.rw_flag,
        rollback: query.rollback,
        is_service: query.is_service,
        shard_id: query.shard_id,
        tx_number: query.tx_number_in_block,
    }
}
