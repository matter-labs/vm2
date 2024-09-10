use std::cmp::Ordering;

use primitive_types::{H160, U256};
use zksync_vm2_interface::*;

use crate::{
    callframe::{Callframe, NearCallFrame},
    decommit::is_kernel,
    predication::{self, Predicate},
    VirtualMachine,
};

impl<T, W> StateInterface for VirtualMachine<T, W> {
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
        CallframeWrapper {
            frame: &mut self.state.current_frame,
            near_call: None,
        }
    }

    fn callframe(&mut self, mut n: usize) -> impl CallframeInterface + '_ {
        for far_frame in std::iter::once(&mut self.state.current_frame)
            .chain(self.state.previous_frames.iter_mut().rev())
        {
            match n.cmp(&far_frame.near_calls.len()) {
                Ordering::Less => {
                    return CallframeWrapper {
                        frame: far_frame,
                        near_call: Some(n),
                    }
                }
                Ordering::Equal => {
                    return CallframeWrapper {
                        frame: far_frame,
                        near_call: None,
                    }
                }
                Ordering::Greater => n -= far_frame.near_calls.len() + 1,
            }
        }
        panic!("Callframe index out of bounds")
    }

    fn read_heap_byte(&self, heap: HeapId, index: u32) -> u8 {
        self.state.heaps[heap].read_byte(index)
    }

    fn write_heap_byte(&mut self, heap: HeapId, index: u32, byte: u8) {
        self.state.heaps.write_byte(heap, index, byte);
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
            .write_transient_storage(address, slot, value)
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

impl<T, W> CallframeInterface for CallframeWrapper<'_, T, W> {
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
        self.frame.stipend
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

    fn read_code_page(&self, slot: u16) -> U256 {
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

    fn program_counter(&self) -> Option<u16> {
        if let Some(call) = self.near_call_on_top() {
            Some(call.previous_frame_pc)
        } else {
            let offset = unsafe {
                self.frame
                    .pc
                    .offset_from(self.frame.program.instruction(0).unwrap())
            };
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
        self.frame.set_pc_from_u16(value);
    }

    fn exception_handler(&self) -> u16 {
        if let Some(i) = self.near_call {
            self.frame.near_calls[self.frame.near_calls.len() - i - 1].exception_handler
        } else {
            self.frame.exception_handler
        }
    }

    fn set_exception_handler(&mut self, value: u16) {
        if let Some(i) = self.near_call {
            let idx = self.frame.near_calls.len() - i - 1;
            self.frame.near_calls[idx].exception_handler = value;
        } else {
            self.frame.exception_handler = value;
        }
    }
}

impl<T, W> CallframeWrapper<'_, T, W> {
    fn near_call_on_top(&self) -> Option<&NearCallFrame> {
        if self.frame.near_calls.is_empty() || self.near_call == Some(0) {
            None
        } else {
            let index = if let Some(i) = self.near_call {
                i - 1
            } else {
                self.frame.near_calls.len() - 1
            };
            Some(&self.frame.near_calls[index])
        }
    }

    fn near_call_on_top_mut(&mut self) -> Option<&mut NearCallFrame> {
        if self.frame.near_calls.is_empty() || self.near_call == Some(0) {
            None
        } else {
            let index = if let Some(i) = self.near_call {
                i - 1
            } else {
                self.frame.near_calls.len() - 1
            };
            Some(&mut self.frame.near_calls[index])
        }
    }
}

#[cfg(all(test, not(feature = "single_instruction_test")))]
mod test {
    use primitive_types::H160;
    use zkevm_opcode_defs::ethereum_types::Address;
    use zksync_vm2_interface::HeapId;

    use super::*;
    use crate::{initial_decommit, testworld::TestWorld, Instruction, Program, VirtualMachine};

    #[test]
    fn callframe_picking() {
        let program = Program::new(vec![Instruction::from_invalid()], vec![]);

        let address = Address::from_low_u64_be(0x1234567890abcdef);
        let mut world = TestWorld::new(&[(address, program)]);
        let program = initial_decommit(&mut world, address);

        let mut vm = VirtualMachine::new(
            address,
            program.clone(),
            Address::zero(),
            vec![],
            1000,
            crate::Settings {
                default_aa_code_hash: [0; 32],
                evm_interpreter_code_hash: [0; 32],
                hook_address: 0,
            },
        );

        let mut frame_count = 1;

        let add_far_frame = |vm: &mut VirtualMachine<(), TestWorld<()>>, counter: &mut u16| {
            vm.push_frame::<opcodes::Normal>(
                H160::from_low_u64_be(1),
                program.clone(),
                0,
                0,
                *counter,
                false,
                HeapId::from_u32_unchecked(5),
                vm.world_diff.snapshot(),
            );
            *counter += 1;
        };

        let add_near_frame = |vm: &mut VirtualMachine<(), TestWorld<()>>, counter: &mut u16| {
            vm.state
                .current_frame
                .push_near_call(0, *counter, vm.world_diff.snapshot());
            *counter += 1;
        };

        add_far_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);
        add_far_frame(&mut vm, &mut frame_count);
        add_far_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);
        add_near_frame(&mut vm, &mut frame_count);

        for i in 0..frame_count {
            assert_eq!(
                vm.callframe(i as usize).exception_handler(),
                frame_count - i - 1
            );
        }
    }
}
