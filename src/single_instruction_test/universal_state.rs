use u256::{H160, U256};
use zk_evm::{
    reference_impls::event_sink::InMemoryEventSink,
    vm_state::{CallStackEntry, VmLocalState, VmState},
};
use zkevm_opcode_defs::decoding::EncodingModeProduction;

use super::into_zk_evm::{MockDecommitter, MockMemory, MockWorldWrapper, NoOracle};

#[derive(PartialEq, Debug)]
pub struct UniversalVmState {
    registers: [(U256, bool); 15],
    flags: [bool; 3],
    transaction_number: u16,
    context_u128: u128,
    frames: Vec<UniversalVmFrame>,
}

#[derive(PartialEq, Debug)]
pub struct UniversalVmFrame {
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

impl
    From<
        VmState<
            MockWorldWrapper,
            MockMemory,
            InMemoryEventSink,
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
            InMemoryEventSink,
            NoOracle,
            MockDecommitter,
            NoOracle,
            8,
            EncodingModeProduction,
        >,
    ) -> Self {
        zk_evm_state_to_universal(&vm.local_state)
    }
}

fn zk_evm_state_to_universal(vm: &VmLocalState<8, EncodingModeProduction>) -> UniversalVmState {
    let mut current_frame = zk_evm_frame_to_universal(&vm.callstack.current);
    // Most of the current frame doesn't matter if we panic, as it is just thrown away
    // but only sp has proved problematic so far
    if vm.pending_exception {
        current_frame.sp = 0;
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
        frames: vm
            .callstack
            .inner
            .iter()
            .skip(1) // zk_evm requires an unused bottom frame
            .map(zk_evm_frame_to_universal)
            .chain(std::iter::once(current_frame))
            .collect::<Vec<_>>(),
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
