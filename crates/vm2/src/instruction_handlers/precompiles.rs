use primitive_types::U256;
use zksync_vm2_interface::{opcodes, HeapId, Tracer};

use super::{common::boilerplate_ext, ret::spontaneous_panic};
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source},
    instruction::ExecutionStatus,
    precompiles::{PrecompileMemoryReader, KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS},
    Instruction, VirtualMachine, World,
};

#[derive(Debug)]
struct PrecompileAuxData {
    extra_ergs_cost: u32,
    extra_pubdata_cost: u32,
}

impl PrecompileAuxData {
    #[allow(clippy::cast_possible_truncation)]
    fn from_u256(raw_value: U256) -> Self {
        let raw = raw_value.0;
        let extra_ergs_cost = raw[0] as u32;
        let extra_pubdata_cost = (raw[0] >> 32) as u32;

        Self {
            extra_ergs_cost,
            extra_pubdata_cost,
        }
    }
}

#[derive(Debug)]
struct PrecompileCallAbi {
    input_memory_offset: u32,
    input_memory_length: u32,
    output_memory_offset: u32,
    output_memory_length: u32,
    memory_page_to_read: HeapId,
    memory_page_to_write: HeapId,
    precompile_interpreted_data: u64,
}

impl PrecompileCallAbi {
    #[allow(clippy::cast_possible_truncation)]
    fn from_u256(raw_value: U256) -> Self {
        let raw = raw_value.0;
        let input_memory_offset = raw[0] as u32;
        let input_memory_length = (raw[0] >> 32) as u32;
        let output_memory_offset = raw[1] as u32;
        let output_memory_length = (raw[1] >> 32) as u32;
        let memory_page_to_read = HeapId::from_u32_unchecked(raw[2] as u32);
        let memory_page_to_write = HeapId::from_u32_unchecked((raw[2] >> 32) as u32);
        let precompile_interpreted_data = raw[3];

        Self {
            input_memory_offset,
            input_memory_length,
            output_memory_offset,
            output_memory_length,
            memory_page_to_read,
            memory_page_to_write,
            precompile_interpreted_data,
        }
    }
}

fn precompile_call<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate_ext::<opcodes::PrecompileCall, _, _>(
        vm,
        world,
        tracer,
        |vm, args, world, _tracer| {
            // The user gets to decide how much gas to burn
            // This is safe because system contracts are trusted
            let aux_data = PrecompileAuxData::from_u256(Register2::get(args, &mut vm.state));
            let Ok(()) = vm.state.use_gas(aux_data.extra_ergs_cost) else {
                vm.state.current_frame.pc = spontaneous_panic();
                return;
            };

            #[allow(clippy::cast_possible_wrap)]
            {
                vm.world_diff.pubdata.0 += aux_data.extra_pubdata_cost as i32;
            }

            let mut abi = PrecompileCallAbi::from_u256(Register1::get(args, &mut vm.state));
            if abi.memory_page_to_read.as_u32() == 0 {
                abi.memory_page_to_read = vm.state.current_frame.heap;
            }
            if abi.memory_page_to_write.as_u32() == 0 {
                abi.memory_page_to_write = vm.state.current_frame.heap;
            }

            let address_bytes = vm.state.current_frame.address.0;
            let address_low = u16::from_le_bytes([address_bytes[19], address_bytes[18]]);
            let heap_to_read = &vm.state.heaps[abi.memory_page_to_read];
            let (read_offset, read_len) =
                if address_low == KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS {
                    // keccak is the only precompile that interprets input offset / length as bytes
                    (abi.input_memory_offset, abi.input_memory_length)
                } else {
                    // Everything else interprets input offset / length as words
                    (abi.input_memory_offset * 32, abi.input_memory_length * 32)
                };
            let output = world.call_precompile(
                address_low,
                abi.precompile_interpreted_data,
                PrecompileMemoryReader::new(heap_to_read, read_offset, read_len),
            );

            let mut write_offset = abi.output_memory_offset * 32;
            for i in 0..output.len.min(abi.output_memory_length) {
                vm.state.heaps.write_u256(
                    abi.memory_page_to_write,
                    write_offset,
                    output.buffer[i as usize],
                );
                write_offset += 32;
            }
            Register1::set(args, &mut vm.state, 1.into());
        },
    )
}

impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a [`PrecompileCall`](opcodes::PrecompileCall) instruction with the provided params.
    pub fn from_precompile_call(
        abi: Register1,
        burn: Register2,
        out: Register1,
        arguments: Arguments,
    ) -> Self {
        Self {
            arguments: arguments
                .write_source(&abi)
                .write_source(&burn)
                .write_destination(&out),
            handler: precompile_call,
        }
    }
}
