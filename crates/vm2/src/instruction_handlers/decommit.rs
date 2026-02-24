use primitive_types::U256;
use zkevm_opcode_defs::{BlobSha256Format, ContractCodeSha256Format, VersionedHashLen32};
use zksync_vm2_interface::{opcodes, Tracer};

use super::common::boilerplate_ext;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source},
    fat_pointer::FatPointer,
    instruction::ExecutionStatus,
    Instruction, VirtualMachine, World,
};

fn decommit<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    boilerplate_ext::<opcodes::Decommit, _, _>(vm, world, tracer, |vm, args, world, tracer| {
        let code_hash = Register1::get(args, &mut vm.state);
        let extra_cost = Register2::get(args, &mut vm.state).low_u32();

        let mut buffer = [0u8; 32];
        code_hash.to_big_endian(&mut buffer);

        let preimage_len_in_bytes =
            zkevm_opcode_defs::system_params::NEW_KERNEL_FRAME_MEMORY_STIPEND;

        if vm.state.use_gas(extra_cost).is_err()
            || (!ContractCodeSha256Format::is_valid(&buffer)
                && !BlobSha256Format::is_valid(&buffer))
        {
            Register1::set(args, &mut vm.state, U256::zero());
            return;
        }

        let (program, is_fresh) = vm.world_diff.decommit_opcode(world, tracer, code_hash);
        if !is_fresh {
            vm.state.current_frame.gas += extra_cost;
        }

        let heap = if is_fresh {
            let heap = vm.state.heaps.allocate_with_content(program.as_ref());
            vm.world_diff.set_decommit_page(code_hash, heap);
            heap
        } else {
            vm.world_diff
                .decommit_page(code_hash)
                .expect("decommit page must exist for non-fresh hash")
        };

        // Decommit page mapping lives for the whole VM run, so nested-frame decommits
        // must pin pages in the bootloader frame rather than in the current frame.
        let heaps_to_keep_alive =
            if let Some(bootloader_frame) = vm.state.previous_frames.first_mut() {
                &mut bootloader_frame.heaps_i_am_keeping_alive
            } else {
                &mut vm.state.current_frame.heaps_i_am_keeping_alive
            };
        if !heaps_to_keep_alive.contains(&heap) {
            heaps_to_keep_alive.push(heap);
        }

        let value = FatPointer {
            offset: 0,
            memory_page: heap,
            start: 0,
            length: preimage_len_in_bytes,
        };
        let value = value.into_u256();
        Register1::set_fat_ptr(args, &mut vm.state, value);
    })
}

impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a [`Decommit`](opcodes::Decommit) instruction with the provided params.
    pub fn from_decommit(
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
            handler: decommit,
        }
    }
}
