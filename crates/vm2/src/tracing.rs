use std::cmp::Ordering;

use primitive_types::{H160, U256};
use zksync_vm2_interface::{
    CallframeInterface, Event, Flags, GlobalStateInterface, HeapId, L2ToL1Log, StateInterface,
    Tracer,
};

use crate::{
    callframe::{Callframe, NearCallFrame},
    decommit::is_kernel,
    predication::{self, Predicate},
    VirtualMachine, World,
};

impl<T: Tracer, W: World<T>> StateInterface for VirtualMachine<T, W> {
    fn read_register(&self, register: u8) -> (U256, bool) {
        (
            self.state.registers[register as usize],
            self.state.register_pointer_flags & (1 << register) != 0,
        )
    }

    fn set_register(&mut self, register: u8, value: U256, is_pointer: bool) {
        self.state.registers[register as usize] = value;

        self.state.register_pointer_flags &= !(1 << register);
        self.state.register_pointer_flags |= u16::from(is_pointer) << register;
    }

    fn number_of_callframes(&self) -> usize {
        self.state
            .previous_frames
            .iter()
            .map(|frame| frame.near_calls.len() + 1)
            .sum::<usize>()
            + self.state.current_frame.near_calls.len()
            + 1
    }

    fn current_frame(&mut self) -> impl CallframeInterface + '_ {
        let near_call = self.state.current_frame.near_calls.len().checked_sub(1);
        CallframeWrapper {
            frame: &mut self.state.current_frame,
            near_call,
        }
    }

    fn callframe(&mut self, mut n: usize) -> impl CallframeInterface + '_ {
        for far_frame in std::iter::once(&mut self.state.current_frame)
            .chain(self.state.previous_frames.iter_mut().rev())
        {
            let near_calls = far_frame.near_calls.len();
            match n.cmp(&near_calls) {
                Ordering::Less => {
                    return CallframeWrapper {
                        frame: far_frame,
                        near_call: Some(near_calls - 1 - n),
                    }
                }
                Ordering::Equal => {
                    return CallframeWrapper {
                        frame: far_frame,
                        near_call: None,
                    }
                }
                Ordering::Greater => n -= near_calls + 1,
            }
        }
        panic!("Callframe index out of bounds")
    }

    fn read_heap_byte(&self, heap: HeapId, index: u32) -> u8 {
        self.state.heaps[heap].read_byte(index)
    }

    fn read_heap_u256(&self, heap: HeapId, index: u32) -> U256 {
        self.state.heaps[heap].read_u256(index)
    }

    fn write_heap_u256(&mut self, heap: HeapId, index: u32, value: U256) {
        self.state.heaps.write_u256(heap, index, value);
    }

    fn flags(&self) -> Flags {
        let flags = &self.state.flags;
        Flags {
            less_than: Predicate::IfLT.satisfied(flags),
            greater: Predicate::IfGT.satisfied(flags),
            equal: Predicate::IfEQ.satisfied(flags),
        }
    }

    fn set_flags(&mut self, flags: Flags) {
        self.state.flags = predication::Flags::new(flags.less_than, flags.equal, flags.greater);
    }

    fn transaction_number(&self) -> u16 {
        self.state.transaction_number
    }

    fn set_transaction_number(&mut self, value: u16) {
        self.state.transaction_number = value;
    }

    fn context_u128_register(&self) -> u128 {
        self.state.context_u128
    }

    fn set_context_u128_register(&mut self, value: u128) {
        self.state.context_u128 = value;
    }

    fn get_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        self.world_diff
            .get_storage_state()
            .iter()
            .map(|(key, value)| (*key, *value))
    }

    fn get_transient_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        self.world_diff
            .get_transient_storage_state()
            .iter()
            .map(|(key, value)| (*key, *value))
    }

    fn get_transient_storage(&self, address: H160, slot: U256) -> U256 {
        self.world_diff
            .get_transient_storage_state()
            .get(&(address, slot))
            .copied()
            .unwrap_or_default()
    }

    fn write_transient_storage(&mut self, address: H160, slot: U256, value: U256) {
        self.world_diff
            .write_transient_storage(address, slot, value);
    }

    fn events(&self) -> impl Iterator<Item = Event> {
        self.world_diff.events().iter().copied()
    }

    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log> {
        self.world_diff.l2_to_l1_logs().iter().copied()
    }

    fn pubdata(&self) -> i32 {
        self.world_diff.pubdata()
    }

    fn set_pubdata(&mut self, value: i32) {
        self.world_diff.pubdata.0 = value;
    }
}

struct CallframeWrapper<'a, T, W> {
    frame: &'a mut Callframe<T, W>,
    near_call: Option<usize>,
}

impl<T: Tracer, W: World<T>> CallframeInterface for CallframeWrapper<'_, T, W> {
    fn address(&self) -> H160 {
        self.frame.address
    }

    fn set_address(&mut self, address: H160) {
        self.frame.address = address;
        self.frame.is_kernel = is_kernel(address);
    }

    fn code_address(&self) -> H160 {
        self.frame.code_address
    }

    fn set_code_address(&mut self, address: H160) {
        self.frame.code_address = address;
    }

    fn caller(&self) -> H160 {
        self.frame.caller
    }

    fn set_caller(&mut self, address: H160) {
        self.frame.caller = address;
    }

    fn is_static(&self) -> bool {
        self.frame.is_static
    }

    fn is_kernel(&self) -> bool {
        self.frame.is_kernel
    }

    fn stipend(&self) -> u32 {
        0 // stipend is no longer used
    }

    fn context_u128(&self) -> u128 {
        self.frame.context_u128
    }

    fn set_context_u128(&mut self, value: u128) {
        self.frame.context_u128 = value;
    }

    fn read_stack(&self, index: u16) -> (U256, bool) {
        (
            self.frame.stack.get(index),
            self.frame.stack.get_pointer_flag(index),
        )
    }

    fn write_stack(&mut self, index: u16, value: U256, is_pointer: bool) {
        self.frame.stack.set(index, value);
        if is_pointer {
            self.frame.stack.set_pointer_flag(index);
        } else {
            self.frame.stack.clear_pointer_flag(index);
        }
    }

    fn heap(&self) -> HeapId {
        self.frame.heap
    }

    fn heap_bound(&self) -> u32 {
        self.frame.heap_size
    }

    fn set_heap_bound(&mut self, value: u32) {
        self.frame.heap_size = value;
    }

    fn aux_heap(&self) -> HeapId {
        self.frame.aux_heap
    }

    fn aux_heap_bound(&self) -> u32 {
        self.frame.aux_heap_size
    }

    fn set_aux_heap_bound(&mut self, value: u32) {
        self.frame.aux_heap_size = value;
    }

    fn read_contract_code(&self, slot: u16) -> U256 {
        self.frame.program.code_page()[slot as usize]
    }

    // The following methods are affected by near calls

    fn is_near_call(&self) -> bool {
        self.near_call.is_some()
    }

    fn gas(&self) -> u32 {
        if let Some(call) = self.near_call_on_top() {
            call.previous_frame_gas
        } else {
            self.frame.gas
        }
    }

    fn set_gas(&mut self, new_gas: u32) {
        if let Some(call) = self.near_call_on_top_mut() {
            call.previous_frame_gas = new_gas;
        } else {
            self.frame.gas = new_gas;
        }
    }

    fn stack_pointer(&self) -> u16 {
        if let Some(call) = self.near_call_on_top() {
            call.previous_frame_sp
        } else {
            self.frame.sp
        }
    }

    fn set_stack_pointer(&mut self, value: u16) {
        if let Some(call) = self.near_call_on_top_mut() {
            call.previous_frame_sp = value;
        } else {
            self.frame.sp = value;
        }
    }

    // we don't expect the VM to run on 16-bit machines, and sign loss / wrap is checked
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    fn program_counter(&self) -> Option<u16> {
        if let Some(call) = self.near_call_on_top() {
            Some(call.previous_frame_pc)
        } else {
            let offset = self.frame.get_raw_pc();
            if offset < 0
                || offset > u16::MAX as isize
                || self.frame.program.instruction(offset as u16).is_none()
            {
                None
            } else {
                Some(offset as u16)
            }
        }
    }

    fn set_program_counter(&mut self, value: u16) {
        if let Some(call) = self.near_call_on_top_mut() {
            call.previous_frame_pc = value;
        } else {
            self.frame.set_pc_from_u16(value);
        }
    }

    fn exception_handler(&self) -> u16 {
        if let Some(i) = self.near_call {
            self.frame.near_calls[i].exception_handler
        } else {
            self.frame.exception_handler
        }
    }

    fn set_exception_handler(&mut self, value: u16) {
        if let Some(i) = self.near_call {
            self.frame.near_calls[i].exception_handler = value;
        } else {
            self.frame.exception_handler = value;
        }
    }
}

impl<T, W> CallframeWrapper<'_, T, W> {
    fn near_call_on_top(&self) -> Option<&NearCallFrame> {
        let index = self.near_call.map_or(0, |i| i + 1);
        self.frame.near_calls.get(index)
    }

    fn near_call_on_top_mut(&mut self) -> Option<&mut NearCallFrame> {
        let index = self.near_call.map_or(0, |i| i + 1);
        self.frame.near_calls.get_mut(index)
    }
}

pub(crate) struct VmAndWorld<'a, T, W> {
    pub vm: &'a mut VirtualMachine<T, W>,
    pub world: &'a mut W,
}

impl<T: Tracer, W: World<T>> GlobalStateInterface for VmAndWorld<'_, T, W> {
    fn get_storage(&mut self, address: H160, slot: U256) -> U256 {
        self.vm
            .world_diff
            .just_read_storage(self.world, address, slot)
    }
}

// This impl just forwards all calls to the VM part of VmAndWorld
impl<T: Tracer, W: World<T>> StateInterface for VmAndWorld<'_, T, W> {
    fn read_register(&self, register: u8) -> (U256, bool) {
        self.vm.read_register(register)
    }
    fn set_register(&mut self, register: u8, value: U256, is_pointer: bool) {
        self.vm.set_register(register, value, is_pointer);
    }
    fn current_frame(&mut self) -> impl CallframeInterface + '_ {
        self.vm.current_frame()
    }
    fn number_of_callframes(&self) -> usize {
        self.vm.number_of_callframes()
    }
    fn callframe(&mut self, n: usize) -> impl CallframeInterface + '_ {
        self.vm.callframe(n)
    }
    fn read_heap_byte(&self, heap: HeapId, offset: u32) -> u8 {
        self.vm.read_heap_byte(heap, offset)
    }
    fn read_heap_u256(&self, heap: HeapId, offset: u32) -> U256 {
        self.vm.read_heap_u256(heap, offset)
    }
    fn write_heap_u256(&mut self, heap: HeapId, offset: u32, value: U256) {
        self.vm.write_heap_u256(heap, offset, value);
    }
    fn flags(&self) -> Flags {
        self.vm.flags()
    }
    fn set_flags(&mut self, flags: Flags) {
        self.vm.set_flags(flags);
    }
    fn transaction_number(&self) -> u16 {
        self.vm.transaction_number()
    }
    fn set_transaction_number(&mut self, value: u16) {
        self.vm.set_transaction_number(value);
    }
    fn context_u128_register(&self) -> u128 {
        self.vm.context_u128_register()
    }
    fn set_context_u128_register(&mut self, value: u128) {
        self.vm.set_context_u128_register(value);
    }
    fn get_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        self.vm.get_storage_state()
    }
    fn get_transient_storage_state(&self) -> impl Iterator<Item = ((H160, U256), U256)> {
        self.vm.get_transient_storage_state()
    }
    fn get_transient_storage(&self, address: H160, slot: U256) -> U256 {
        self.vm.get_transient_storage(address, slot)
    }
    fn write_transient_storage(&mut self, address: H160, slot: U256, value: U256) {
        self.vm.write_transient_storage(address, slot, value);
    }
    fn events(&self) -> impl Iterator<Item = Event> {
        self.vm.events()
    }
    fn l2_to_l1_logs(&self) -> impl Iterator<Item = L2ToL1Log> {
        self.vm.l2_to_l1_logs()
    }
    fn pubdata(&self) -> i32 {
        self.vm.pubdata()
    }
    fn set_pubdata(&mut self, value: i32) {
        self.vm.set_pubdata(value);
    }
}

#[cfg(all(test, not(feature = "single_instruction_test")))]
mod test {
    use primitive_types::H160;
    use zkevm_opcode_defs::ethereum_types::Address;
    use zksync_vm2_interface::opcodes;

    use super::*;
    use crate::{
        testonly::{initial_decommit, TestWorld},
        Instruction, Program, VirtualMachine,
    };

    #[test]
    fn callframe_picking() {
        let program = Program::from_raw(vec![Instruction::from_invalid()], vec![]);

        let address = Address::from_low_u64_be(0x_1234_5678_90ab_cdef);
        let mut world = TestWorld::new(&[(address, program)]);
        let program = initial_decommit(&mut world, address);

        let mut vm = VirtualMachine::new(
            address,
            program.clone(),
            Address::zero(),
            &[],
            1000,
            crate::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        vm.state.current_frame.gas = 0;
        vm.state.current_frame.exception_handler = 0;
        let mut frame_count = 1;

        let add_far_frame = |vm: &mut VirtualMachine<(), TestWorld<()>>, counter: &mut u16| {
            vm.push_frame::<opcodes::Normal>(
                H160::from_low_u64_be(1),
                program.clone(),
                (*counter).into(),
                *counter,
                false,
                false,
                HeapId::from_u32_unchecked(5),
                vm.world_diff.snapshot(),
            );
            assert_eq!(vm.current_frame().gas(), (*counter).into());
            *counter += 1;
        };

        let add_near_frame = |vm: &mut VirtualMachine<(), TestWorld<()>>, counter: &mut u16| {
            let count_u32 = (*counter).into();
            vm.state.current_frame.gas += count_u32;
            vm.state
                .current_frame
                .push_near_call(count_u32, *counter, vm.world_diff.snapshot());
            assert_eq!(vm.current_frame().gas(), (*counter).into());
            *counter += 1;
        };

        add_far_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);
        add_far_frame(&mut vm, &mut frame_count);
        add_far_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);

        for (fwd, rev) in (0..frame_count.into()).zip((0..frame_count).rev()) {
            assert_eq!(vm.callframe(fwd).exception_handler(), rev);
            assert_eq!(vm.callframe(fwd).gas(), rev.into());
        }
    }
}
