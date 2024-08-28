use super::common::instruction_boilerplate;
use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source},
    fat_pointer::FatPointer,
    instruction::ExecutionStatus,
    Instruction, VirtualMachine, World,
};
use eravm_stable_interface::{opcodes, Tracer};
use u256::U256;
use zkevm_opcode_defs::{BlobSha256Format, ContractCodeSha256Format, VersionedHashLen32};

fn decommit<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) -> ExecutionStatus {
    instruction_boilerplate::<opcodes::Decommit, _, _>(vm, world, tracer, |vm, args, world| {
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

        let (program, is_fresh) = vm.world_diff.decommit_opcode(world, code_hash);
        if !is_fresh {
            vm.state.current_frame.gas += extra_cost;
        }

        let heap = vm.state.heaps.allocate_with_content(program.as_ref());
        vm.state.current_frame.heaps_i_am_keeping_alive.push(heap);

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
