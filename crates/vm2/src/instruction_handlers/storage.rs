use zksync_vm2_interface::{opcodes, Tracer};

use super::common::{boilerplate, boilerplate_ext};
use crate::{
    addressing_modes::{
        Arguments, Destination, Register1, Register2, Source, SLOAD_COST, SSTORE_COST,
    },
    Instruction, VirtualMachine, World,
};

fn sstore<T: Tracer, W: World<T>>(vm: &mut VirtualMachine<T, W>, world: &mut W, tracer: &mut T) {
    boilerplate_ext::<opcodes::StorageWrite, _, _>(vm, world, tracer, |vm, args, world, tracer| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        let refund =
            vm.world_diff
                .write_storage(world, tracer, vm.state.current_frame.address, key, value);

        assert!(refund <= SSTORE_COST);
        vm.state.current_frame.gas += refund;
    });
}

fn sstore_transient<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) {
    boilerplate::<opcodes::TransientStorageWrite, _, _>(vm, world, tracer, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let value = Register2::get(args, &mut vm.state);

        vm.world_diff
            .write_transient_storage(vm.state.current_frame.address, key, value);
    });
}

fn sload<T: Tracer, W: World<T>>(vm: &mut VirtualMachine<T, W>, world: &mut W, tracer: &mut T) {
    boilerplate_ext::<opcodes::StorageRead, _, _>(vm, world, tracer, |vm, args, world, tracer| {
        let key = Register1::get(args, &mut vm.state);

        let (value, refund) =
            vm.world_diff
                .read_storage(world, tracer, vm.state.current_frame.address, key);

        assert!(refund <= SLOAD_COST);
        vm.state.current_frame.gas += refund;

        Register1::set(args, &mut vm.state, value);
    });
}

fn sload_transient<T: Tracer, W: World<T>>(
    vm: &mut VirtualMachine<T, W>,
    world: &mut W,
    tracer: &mut T,
) {
    boilerplate::<opcodes::TransientStorageRead, _, _>(vm, world, tracer, |vm, args| {
        let key = Register1::get(args, &mut vm.state);
        let value = vm
            .world_diff
            .read_transient_storage(vm.state.current_frame.address, key);

        Register1::set(args, &mut vm.state, value);
    });
}

impl<T: Tracer, W: World<T>> Instruction<T, W> {
    /// Creates a [`StorageWrite`](opcodes::StorageWrite) instruction with the provided params.
    pub fn from_storage_write(src1: Register1, src2: Register2, arguments: Arguments) -> Self {
        Self {
            handler: sstore,
            arguments: arguments.write_source(&src1).write_source(&src2),
        }
    }

    /// Creates a [`TransientStorageWrite`](opcodes::TransientStorageWrite) instruction with the provided params.
    pub fn from_transient_storage_write(
        src1: Register1,
        src2: Register2,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: sstore_transient,
            arguments: arguments.write_source(&src1).write_source(&src2),
        }
    }

    /// Creates a [`StorageRead`](opcodes::StorageRead) instruction with the provided params.
    pub fn from_storage_read(src: Register1, dst: Register1, arguments: Arguments) -> Self {
        Self {
            handler: sload,
            arguments: arguments.write_source(&src).write_destination(&dst),
        }
    }

    /// Creates a [`TransientStorageRead`](opcodes::TransientStorageRead) instruction with the provided params.
    pub fn from_transient_storage_read(
        src: Register1,
        dst: Register1,
        arguments: Arguments,
    ) -> Self {
        Self {
            handler: sload_transient,
            arguments: arguments.write_source(&src).write_destination(&dst),
        }
    }
}
