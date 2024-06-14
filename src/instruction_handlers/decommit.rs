use u256::{H256, U256};
use zkevm_opcode_defs::{
    BlobSha256Format, ContractCodeSha256Format, VersionedHashHeader, VersionedHashLen32,
    VersionedHashNormalizedPreimage,
};

use crate::{
    addressing_modes::{Addressable, Arguments, Destination, Register1, Register2, Source},
    decommit::UnpaidDecommit,
    fat_pointer::FatPointer,
    instruction::InstructionResult,
    Instruction, VirtualMachine, World,
};

use super::{common::instruction_boilerplate_with_panic, HeapInterface};

pub fn u256_to_h256(num: U256) -> H256 {
    let mut bytes = [0u8; 32];
    num.to_big_endian(&mut bytes);
    H256::from_slice(&bytes)
}

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
            let extra_cost = Register2::get(args, &mut vm.state).low_u32();
            let code_hash = Register1::get(args, &mut vm.state);

            /*
            let unpaid_decommit = vm.world_diff.decommit(
                world,
                CodeInfo::CodeHash(code_hash),
                vm.settings.default_aa_code_hash,
                vm.settings.evm_interpreter_code_hash,
                vm.state.in_kernel_mode(),
            );

            if unpaid_decommit.as_ref().unwrap().0.is_initial() {
                vm.state.current_frame.gas.saturating_sub(extra_cost);
            }
            */

            let unpaid_decommit = UnpaidDecommit {
                cost: 1000,
                code_key: code_hash,
                is_initial: true,
            };

            let decommit_result = vm.world_diff.pay_for_decommit(
                world,
                unpaid_decommit,
                &mut vm.state.current_frame.gas,
            );

            let heap = vm.state.heaps.allocate();
            let mut length = decommit_result
                .as_ref()
                .unwrap()
                .code_page()
                .as_ref()
                .len()
                .try_into()
                .unwrap();

            length *= 32;

            for (i, word) in decommit_result
                .unwrap()
                .code_page()
                .as_ref()
                .iter()
                .enumerate()
            {
                let mut bytes = [0; 32];
                word.to_big_endian(&mut bytes);
                let h: H256 = H256::from(bytes);

                vm.state.heaps[heap].write_u256((i * 32) as u32, *word);
            }

            let value = FatPointer {
                offset: 0,
                memory_page: heap,
                start: 0,
                length,
            };
            dbg!(&value);
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
