use crate::{callframe::Callframe, instruction_handlers::HeapInterface, VirtualMachine};
use eravm_stable_interface::*;

impl StateInterface for VirtualMachine {
    fn read_register(&self, register: u8) -> (u256::U256, bool) {
        (
            self.state.registers[register as usize],
            self.state.register_pointer_flags & (1 << register) != 0,
        )
    }

    fn set_register(&mut self, register: u8, value: u256::U256, is_pointer: bool) {
        self.state.registers[register as usize] = value;

        self.state.register_pointer_flags &= !(1 << register);
        self.state.register_pointer_flags |= u16::from(is_pointer) << register;
    }

    fn number_of_callframes(&self) -> usize {
        self.state.previous_frames.len() + 1
    }

    fn callframe(&mut self, n: usize) -> &mut impl CallframeInterface {
        if n == 0 {
            &mut self.state.current_frame
        } else {
            &mut self.state.previous_frames[n - 1].1
        }
    }

    fn read_heap_byte(&self, heap: HeapId, index: u32) -> u8 {
        self.state.heaps[heap]
    }

    fn write_heap_byte(&mut self, heap: HeapId, index: u32, byte: u8) {
        todo!()
    }

    fn flags(&self) -> Flags {
        todo!()
    }

    fn set_flags(&mut self, flags: Flags) {
        todo!()
    }

    fn transaction_number(&self) -> u16 {
        todo!()
    }

    fn set_transaction_number(&mut self, value: u16) {
        todo!()
    }

    fn context_u128_register(&self) -> u128 {
        todo!()
    }

    fn set_context_u128_register(&mut self, value: u128) {
        todo!()
    }

    fn get_storage_state(&self) -> impl Iterator<Item = ((u256::H160, u256::U256), u256::U256)> {
        self.world_diff
            .get_storage_state()
            .iter()
            .map(|(key, value)| (*key, *value))
    }

    fn get_storage(&self, address: u256::H160, slot: u256::U256) -> Option<(u256::U256, u32)> {
        todo!()
    }

    fn get_storage_initial_value(&self, address: u256::H160, slot: u256::U256) -> u256::U256 {
        todo!()
    }

    fn write_storage(&mut self, address: u256::H160, slot: u256::U256, value: u256::U256) {
        todo!()
    }

    fn get_transient_storage_state(
        &self,
    ) -> impl Iterator<Item = ((u256::H160, u256::U256), u256::U256)> {
        self.world_diff
            .get_transient_storage_state()
            .iter()
            .map(|(key, value)| (*key, *value))
    }

    fn get_transient_storage(&self, address: u256::H160, slot: u256::U256) -> u256::U256 {
        todo!()
    }

    fn write_transient_storage(
        &mut self,
        address: u256::H160,
        slot: u256::U256,
        value: u256::U256,
    ) {
        todo!()
    }

    fn events(&self) -> impl Iterator<Item = Event> {
        self.world_diff.events().iter().map(|event| Event {
            key: event.key,
            value: event.value,
            is_first: event.is_first,
            shard_id: event.shard_id,
            tx_number: event.tx_number,
        })
    }

    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log> {
        self.world_diff.l2_to_l1_logs().iter().map(|log| L2ToL1Log {
            address: log.address,
            key: log.key,
            value: log.value,
            is_service: log.is_service,
            shard_id: log.shard_id,
            tx_number: log.tx_number,
        })
    }

    fn pubdata(&self) -> i32 {
        todo!()
    }

    fn set_pubdata(&mut self, value: i32) {
        todo!()
    }

    fn run_arbitrary_code(code: &[u64]) {
        todo!()
    }

    fn static_heap(&self) -> HeapId {
        todo!()
    }
}

impl CallframeInterface for Callframe {
    fn address(&self) -> u256::H160 {
        todo!()
    }

    fn set_address(&mut self, address: u256::H160) {
        todo!()
    }

    fn code_address(&self) -> u256::H160 {
        todo!()
    }

    fn set_code_address(&mut self, address: u256::H160) {
        todo!()
    }

    fn caller(&self) -> u256::H160 {
        todo!()
    }

    fn set_caller(&mut self, address: u256::H160) {
        todo!()
    }

    fn program_counter(&self) -> Option<u16> {
        todo!()
    }

    fn set_program_counter(&mut self, value: u16) {
        todo!()
    }

    fn exception_handler(&self) -> u16 {
        todo!()
    }

    fn is_static(&self) -> bool {
        todo!()
    }

    fn gas(&self) -> u32 {
        todo!()
    }

    fn set_gas(&mut self, new_gas: u32) {
        todo!()
    }

    fn stipend(&self) -> u32 {
        todo!()
    }

    fn context_u128(&self) -> u128 {
        todo!()
    }

    fn set_context_u128(&mut self, value: u128) {
        todo!()
    }

    fn is_near_call(&self) -> bool {
        todo!()
    }

    fn read_stack(&self, register: u16) -> (u256::U256, bool) {
        todo!()
    }

    fn write_stack(&mut self, register: u16, value: u256::U256, is_pointer: bool) {
        todo!()
    }

    fn stack_pointer(&self) -> u16 {
        todo!()
    }

    fn set_stack_pointer(&mut self, value: u16) {
        todo!()
    }

    fn heap(&self) -> HeapId {
        todo!()
    }

    fn heap_bound(&self) -> u32 {
        todo!()
    }

    fn set_heap_bound(&mut self, value: u32) {
        todo!()
    }

    fn aux_heap(&self) -> HeapId {
        todo!()
    }

    fn aux_heap_bound(&self) -> u32 {
        todo!()
    }

    fn set_aux_heap_bound(&mut self, value: u32) {
        todo!()
    }

    fn read_code_page(&self, slot: u16) -> u256::U256 {
        todo!()
    }
}
