use u256::U256;

use super::{
    common::{instruction_boilerplate, instruction_boilerplate_with_panic},
    PANIC,
};
use crate::{
    addressing_modes::{
        Arguments, Destination, Register1, Register2, Source, SLOAD_COST, SSTORE_COST,
    },
    compression::compress_with_best_strategy,
    instruction::InstructionResult,
    modified_world::is_storage_key_free,
    Instruction, Predicate, VirtualMachine,
};

/// The number of bytes being used for state diff enumeration indices. Applicable to repeated writes.
pub const BYTES_PER_ENUMERATION_INDEX: u8 = 4;
/// The number of bytes being used for state diff derived keys. Applicable to initial writes.
pub const BYTES_PER_DERIVED_KEY: u8 = 32;

/// Returns the number of bytes needed to publish a slot.
// Since we need to publish the state diffs onchain, for each of the updated storage slot
// we basically need to publish the following pair: `(<storage_key, compressed_new_value>)`.
// For key we use the following optimization:
//   - The first time we publish it, we use 32 bytes.
//         Then, we remember a 8-byte id for this slot and assign it to it. We call this initial write.
//   - The second time we publish it, we will use the 4/5 byte representation of this 8-byte instead of the 32
//     bytes of the entire key.
// For value compression, we use a metadata byte which holds the length of the value and the operation from the
// previous state to the new state, and the compressed value. The maximum for this is 33 bytes.
// Total bytes for initial writes then becomes 65 bytes and repeated writes becomes 38 bytes.
fn get_pubdata_price_bytes(initial_value: U256, final_value: U256, is_initial: bool) -> u32 {
    // TODO (SMA-1702): take into account the content of the log query, i.e. values that contain mostly zeroes
    // should cost less.
    let compressed_value_size =
        compress_with_best_strategy(initial_value, final_value).len() as u32;

    if is_initial {
        (BYTES_PER_DERIVED_KEY as u32) + compressed_value_size
    } else {
        (BYTES_PER_ENUMERATION_INDEX as u32) + compressed_value_size
    }
}

fn base_price_for_write_query(vm: &mut VirtualMachine, key: U256, new_value: U256) -> u32 {
    let initial_value = vm
        .world
        .world
        .read_storage(vm.state.current_frame.address, key);

    let is_initial = vm
        .world
        .is_write_initial(vm.state.current_frame.address, key);

    if is_storage_key_free(&vm.state.current_frame.address, &key) || initial_value == new_value {
        return 0;
    }

    get_pubdata_price_bytes(initial_value, new_value, is_initial)
}

fn sstore(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate_with_panic(vm, instruction, |vm, args, continue_normally| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        let to_pay_by_user = base_price_for_write_query(vm, key, value);
        let prepaid = vm
            .world
            .prepaid_for_write(vm.state.current_frame.address, key);

        // Note, that the diff may be negative, e.g. in case the new write returns to the previous value.
        let diff = (to_pay_by_user as i32) - (prepaid as i32);
        vm.state.current_frame.total_pubdata_spent += diff;

        vm.world
            .insert_prepaid_for_write(vm.state.current_frame.address, key, to_pay_by_user);

        if vm.state.current_frame.is_static {
            return Ok(&PANIC);
        }

        let refund = vm
            .world
            .write_storage(vm.state.current_frame.address, key, value);

        assert!(refund <= SSTORE_COST);
        vm.state.current_frame.gas += refund;

        continue_normally
    })
}

fn sload(vm: &mut VirtualMachine, instruction: *const Instruction) -> InstructionResult {
    instruction_boilerplate(vm, instruction, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let (value, refund) = vm.world.read_storage(vm.state.current_frame.address, key);

        assert!(refund <= SLOAD_COST);
        vm.state.current_frame.gas += refund;

        Register1::set(args, &mut vm.state, value);
    })
}

impl Instruction {
    #[inline(always)]
    pub fn from_sstore(src1: Register1, src2: Register2, predicate: Predicate) -> Self {
        Self {
            handler: sstore,
            arguments: Arguments::new(predicate, SSTORE_COST)
                .write_source(&src1)
                .write_source(&src2),
        }
    }
}

impl Instruction {
    #[inline(always)]
    pub fn from_sload(src: Register1, dst: Register1, predicate: Predicate) -> Self {
        Self {
            handler: sload,
            arguments: Arguments::new(predicate, SLOAD_COST)
                .write_source(&src)
                .write_destination(&dst),
        }
    }
}
