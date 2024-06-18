use u256::U256;
use zkevm_opcode_defs::{
    BlobSha256Format, ContractCodeSha256Format, VersionedHashHeader, VersionedHashLen32,
    VersionedHashNormalizedPreimage,
};

use crate::{
    addressing_modes::{Arguments, Destination, Register1, Register2, Source},
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    Instruction, VirtualMachine, World,
};

use super::{common::instruction_boilerplate_with_panic, HeapInterface};

fn decommit(
    vm: &mut VirtualMachine,
    instruction: *const Instruction,
    world: &mut dyn World,
) -> InstructionResult {
    instruction_boilerplate_with_panic(
        vm,
        instruction,
        world,
        |vm, args, world, continue_normally| {
            let code_hash = Register1::get(args, &mut vm.state);
            let extra_cost = Register2::get(args, &mut vm.state).low_u32();

            let mut decommit_preimage_format_is_invalid = false;
            let mut buffer = [0u8; 32];
            code_hash.to_big_endian(&mut buffer);
            let mut _decommit_preimage_normalized = VersionedHashNormalizedPreimage::default();

            let mut preimage_len_in_bytes =
                zkevm_opcode_defs::system_params::NEW_KERNEL_FRAME_MEMORY_STIPEND;
            let mut _decommit_header = VersionedHashHeader::default();

            if ContractCodeSha256Format::is_valid(&buffer) {
                let (header, normalized_preimage) =
                    ContractCodeSha256Format::normalize_for_decommitment(&buffer);
                _decommit_header = header;
                _decommit_preimage_normalized = normalized_preimage;
            } else if BlobSha256Format::is_valid(&buffer) {
                let (header, normalized_preimage) =
                    BlobSha256Format::normalize_for_decommitment(&buffer);
                _decommit_header = header;
                _decommit_preimage_normalized = normalized_preimage;
            } else {
                preimage_len_in_bytes = 0;
                decommit_preimage_format_is_invalid = true;
            };

            if vm.state.use_gas(extra_cost).is_err() {
                Register1::set(args, &mut vm.state, U256::zero());
                return continue_normally;
            }

            if decommit_preimage_format_is_invalid {
                let value = U256::zero();
                Register1::set(args, &mut vm.state, value);
                return continue_normally;
            }

            let Some(unpaid_decommit) = vm.world_diff.decommit_opcode(code_hash) else {
                Register1::set(args, &mut vm.state, U256::zero());
                return continue_normally;
            };

            let decommit_result = vm.world_diff.unpaid_decommit(world, unpaid_decommit);

            let heap = vm.state.heaps.allocate();
            let program = &decommit_result.unwrap();
            let decommited_memory = program.code_page().as_ref();
            let mut _length: u32 = decommited_memory.len().try_into().unwrap();
            _length *= 32;

            vm.state.heaps[heap].memset(decommited_memory);

            let value = FatPointer {
                offset: 0,
                memory_page: heap,
                start: 0,
                length: preimage_len_in_bytes,
            };
            let value = value.into_u256();
            Register1::set_fat_ptr(args, &mut vm.state, value);

            continue_normally
        },
    )
}
impl Instruction {
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
