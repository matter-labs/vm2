use crate::{bitset::Bitset, predication::Predicate};
use arbitrary::{Arbitrary, Unstructured};
use enum_dispatch::enum_dispatch;
use u256::U256;

pub(crate) trait Source {
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256;
    fn is_fat_pointer(args: &Arguments, state: &mut impl Addressable) -> bool;
}

pub(crate) trait Destination {
    /// Set this register/stack location to value and clear its pointer flag
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256);

    /// Same as `set` but sets the pointer flag
    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256);
}

/// The part of VM state that addressing modes need to operate on
pub trait Addressable {
    fn registers(&mut self) -> &mut [U256; 16];
    fn register_pointer_flags(&mut self) -> &mut u16;

    fn stack(&mut self) -> &mut [U256; 1 << 16];
    fn stack_pointer_flags(&mut self) -> &mut Bitset;
    fn stack_pointer(&mut self) -> &mut u16;

    fn code_page(&self) -> &[U256];
}

#[enum_dispatch]
pub(crate) trait SourceWriter {
    fn write_source(&self, args: &mut Arguments);
}

impl<T: SourceWriter> SourceWriter for Option<T> {
    fn write_source(&self, args: &mut Arguments) {
        if let Some(x) = self {
            x.write_source(args)
        }
    }
}

#[enum_dispatch]
pub trait DestinationWriter {
    fn write_destination(&self, args: &mut Arguments);
}

impl<T: DestinationWriter> DestinationWriter for Option<T> {
    fn write_destination(&self, args: &mut Arguments) {
        if let Some(x) = self {
            x.write_destination(args)
        }
    }
}

pub struct Arguments {
    source_registers: PackedRegisters,
    destination_registers: PackedRegisters,
    immediate1: u16,
    immediate2: u16,
    pub predicate: Predicate,
    static_gas_cost: u8,
}

pub(crate) const L1_MESSAGE_COST: u32 = 156250;
pub(crate) const SSTORE_COST: u32 = 5511;
pub(crate) const SLOAD_COST: u32 = 2008;
pub(crate) const INVALID_INSTRUCTION_COST: u32 = 4294967295;

impl Arguments {
    pub const fn new(predicate: Predicate, gas_cost: u32) -> Self {
        Self {
            source_registers: PackedRegisters(0),
            destination_registers: PackedRegisters(0),
            immediate1: 0,
            immediate2: 0,
            predicate,
            static_gas_cost: Self::encode_static_gas_cost(gas_cost),
        }
    }

    const fn encode_static_gas_cost(x: u32) -> u8 {
        match x {
            L1_MESSAGE_COST => 1,
            SSTORE_COST => 2,
            SLOAD_COST => 3,
            INVALID_INSTRUCTION_COST => 4,
            1 | 2 | 3 | 4 => panic!("Reserved gas cost values overlap with actual gas costs"),
            x => {
                if x > u8::MAX as u32 {
                    panic!("Gas cost doesn't fit into 8 bits")
                } else {
                    x as u8
                }
            }
        }
    }

    pub(crate) fn get_static_gas_cost(&self) -> u32 {
        match self.static_gas_cost {
            1 => L1_MESSAGE_COST,
            2 => SSTORE_COST,
            3 => SLOAD_COST,
            4 => INVALID_INSTRUCTION_COST,
            x => x.into(),
        }
    }

    pub(crate) fn write_source(mut self, sw: &impl SourceWriter) -> Self {
        sw.write_source(&mut self);
        self
    }

    pub(crate) fn write_destination(mut self, sw: &impl DestinationWriter) -> Self {
        sw.write_destination(&mut self);
        self
    }
}

/// This one should only be used when [Register2] is used as well.
/// It must not be used simultaneously with Absolute/[RelativeStack].
#[derive(Arbitrary)]
pub struct Register1(pub Register);

#[derive(Arbitrary)]
pub struct Register2(pub Register);

impl Source for Register1 {
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256 {
        args.source_registers.register1().value(state)
    }

    fn is_fat_pointer(args: &Arguments, state: &mut impl Addressable) -> bool {
        args.source_registers.register1().pointer_flag(state)
    }
}

impl SourceWriter for Register1 {
    fn write_source(&self, args: &mut Arguments) {
        args.source_registers.set_register1(self.0);
    }
}

impl Source for Register2 {
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256 {
        args.source_registers.register2().value(state)
    }

    fn is_fat_pointer(args: &Arguments, state: &mut impl Addressable) -> bool {
        args.source_registers.register2().pointer_flag(state)
    }
}

impl SourceWriter for Register2 {
    fn write_source(&self, args: &mut Arguments) {
        args.source_registers.set_register2(self.0);
    }
}

impl Destination for Register1 {
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256) {
        args.destination_registers.register1().set(state, value);
    }

    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256) {
        args.destination_registers.register1().set_ptr(state, value);
    }
}

impl DestinationWriter for Register1 {
    fn write_destination(&self, args: &mut Arguments) {
        args.destination_registers.set_register1(self.0)
    }
}

impl Destination for Register2 {
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256) {
        args.destination_registers.register2().set(state, value);
    }

    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256) {
        args.destination_registers.register2().set_ptr(state, value);
    }
}

impl DestinationWriter for Register2 {
    fn write_destination(&self, args: &mut Arguments) {
        args.destination_registers.set_register2(self.0)
    }
}

#[derive(Arbitrary)]
pub struct Immediate1(pub u16);

#[derive(Arbitrary)]
pub struct Immediate2(pub u16);

impl Source for Immediate1 {
    fn get(args: &Arguments, _state: &mut impl Addressable) -> U256 {
        U256([args.immediate1 as u64, 0, 0, 0])
    }

    fn is_fat_pointer(_: &Arguments, _: &mut impl Addressable) -> bool {
        false
    }
}

impl SourceWriter for Immediate1 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate1 = self.0;
    }
}

impl Source for Immediate2 {
    fn get(args: &Arguments, _state: &mut impl Addressable) -> U256 {
        U256([args.immediate2 as u64, 0, 0, 0])
    }

    fn is_fat_pointer(_: &Arguments, _: &mut impl Addressable) -> bool {
        false
    }
}

impl SourceWriter for Immediate2 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate2 = self.0;
    }
}

#[derive(Arbitrary, Clone)]
pub struct RegisterAndImmediate {
    pub immediate: u16,
    pub register: Register,
}

/// Any addressing mode that uses reg + imm in some way.
/// They all encode their parameters in the same way.
trait RegisterPlusImmediate {
    fn inner(&self) -> &RegisterAndImmediate;
}

impl<T: RegisterPlusImmediate> SourceWriter for T {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate1 = self.inner().immediate;
        args.source_registers.set_register1(self.inner().register);
    }
}

impl<T: RegisterPlusImmediate> DestinationWriter for T {
    fn write_destination(&self, args: &mut Arguments) {
        args.immediate2 = self.inner().immediate;
        args.destination_registers
            .set_register1(self.inner().register)
    }
}

trait StackAddressing {
    fn address_for_get(args: &Arguments, state: &mut impl Addressable) -> u16;
    fn address_for_set(args: &Arguments, state: &mut impl Addressable) -> u16;
}

impl<T: StackAddressing> Source for T {
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256 {
        let address = Self::address_for_get(args, state);
        state.stack()[address as usize]
    }

    fn is_fat_pointer(args: &Arguments, state: &mut impl Addressable) -> bool {
        let address = Self::address_for_get(args, state);
        state.stack_pointer_flags().get(address)
    }
}

impl<T: StackAddressing> Destination for T {
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256) {
        let address = Self::address_for_set(args, state);
        state.stack()[address as usize] = value;
        state.stack_pointer_flags().clear(address);
    }

    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256) {
        let address = Self::address_for_set(args, state);
        state.stack()[address as usize] = value;
        state.stack_pointer_flags().set(address);
    }
}

fn source_stack_address(args: &Arguments, state: &mut impl Addressable) -> u16 {
    compute_stack_address(state, args.source_registers.register1(), args.immediate1)
}

pub fn destination_stack_address(args: &Arguments, state: &mut impl Addressable) -> u16 {
    compute_stack_address(
        state,
        args.destination_registers.register1(),
        args.immediate2,
    )
}

/// Computes register + immediate (mod 2^16).
/// Stack addresses are always in that remainder class anyway.
fn compute_stack_address(state: &mut impl Addressable, register: Register, immediate: u16) -> u16 {
    (register.value(state).low_u32() as u16).wrapping_add(immediate)
}

#[derive(Arbitrary)]
pub struct AbsoluteStack(pub RegisterAndImmediate);

impl RegisterPlusImmediate for AbsoluteStack {
    fn inner(&self) -> &RegisterAndImmediate {
        &self.0
    }
}

impl StackAddressing for AbsoluteStack {
    fn address_for_get(args: &Arguments, state: &mut impl Addressable) -> u16 {
        source_stack_address(args, state)
    }

    fn address_for_set(args: &Arguments, state: &mut impl Addressable) -> u16 {
        destination_stack_address(args, state)
    }
}

#[derive(Arbitrary)]
pub struct RelativeStack(pub RegisterAndImmediate);

impl RegisterPlusImmediate for RelativeStack {
    fn inner(&self) -> &RegisterAndImmediate {
        &self.0
    }
}

impl StackAddressing for RelativeStack {
    fn address_for_get(args: &Arguments, state: &mut impl Addressable) -> u16 {
        state
            .stack_pointer()
            .wrapping_sub(source_stack_address(args, state))
    }

    fn address_for_set(args: &Arguments, state: &mut impl Addressable) -> u16 {
        state
            .stack_pointer()
            .wrapping_sub(destination_stack_address(args, state))
    }
}

#[derive(Arbitrary, Clone)]
pub struct AdvanceStackPointer(pub RegisterAndImmediate);

impl RegisterPlusImmediate for AdvanceStackPointer {
    fn inner(&self) -> &RegisterAndImmediate {
        &self.0
    }
}

impl StackAddressing for AdvanceStackPointer {
    fn address_for_get(args: &Arguments, state: &mut impl Addressable) -> u16 {
        let offset = source_stack_address(args, state);
        let sp = state.stack_pointer();
        *sp = sp.wrapping_sub(offset);
        *sp
    }

    fn address_for_set(args: &Arguments, state: &mut impl Addressable) -> u16 {
        let offset = destination_stack_address(args, state);
        let sp = state.stack_pointer();
        let address_to_set = *sp;
        *sp = sp.wrapping_add(offset);
        address_to_set
    }
}

#[derive(Arbitrary)]
pub struct CodePage(pub RegisterAndImmediate);

impl RegisterPlusImmediate for CodePage {
    fn inner(&self) -> &RegisterAndImmediate {
        &self.0
    }
}

impl Source for CodePage {
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256 {
        let address = source_stack_address(args, state);
        state
            .code_page()
            .get(address as usize)
            .cloned()
            .unwrap_or(U256::zero())
    }

    fn is_fat_pointer(_: &Arguments, _: &mut impl Addressable) -> bool {
        false
    }
}

#[derive(Copy, Clone)]
pub struct Register(u8);

impl Register {
    pub fn new(n: u8) -> Self {
        debug_assert!(n < 16);
        Self(n)
    }

    fn value(&self, state: &mut impl Addressable) -> U256 {
        unsafe { *state.registers().get_unchecked(self.0 as usize) }
    }

    fn pointer_flag(&self, state: &mut impl Addressable) -> bool {
        *state.register_pointer_flags() & (1 << self.0) != 0
    }

    fn set(&self, state: &mut impl Addressable, value: U256) {
        if self.0 != 0 {
            unsafe { *state.registers().get_unchecked_mut(self.0 as usize) = value };
            *state.register_pointer_flags() &= !(1 << self.0);
        }
    }

    fn set_ptr(&self, state: &mut impl Addressable, value: U256) {
        if self.0 != 0 {
            unsafe { *state.registers().get_unchecked_mut(self.0 as usize) = value };
            *state.register_pointer_flags() |= 1 << self.0;
        }
    }
}

impl<'a> Arbitrary<'a> for Register {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self, arbitrary::Error> {
        Ok(Register(u.choose_index(16)? as u8))
    }
}

struct PackedRegisters(u8);

impl PackedRegisters {
    fn register1(&self) -> Register {
        Register(self.0 >> 4)
    }
    fn set_register1(&mut self, value: Register) {
        self.0 &= 0xf;
        self.0 |= value.0 << 4;
    }
    fn register2(&self) -> Register {
        Register(self.0 & 0xf)
    }
    fn set_register2(&mut self, value: Register) {
        self.0 &= 0xf0;
        self.0 |= value.0;
    }
}

#[enum_dispatch(SourceWriter)]
#[derive(Arbitrary)]
pub enum AnySource {
    Register1,
    Immediate1,
    AbsoluteStack,
    RelativeStack,
    AdvanceStackPointer,
    CodePage,
}

#[enum_dispatch(SourceWriter)]
#[derive(Arbitrary)]
pub enum RegisterOrImmediate {
    Register1,
    Immediate1,
}

#[derive(Debug)]
pub struct NotRegisterOrImmediate;
impl TryFrom<AnySource> for RegisterOrImmediate {
    type Error = NotRegisterOrImmediate;

    fn try_from(value: AnySource) -> Result<Self, Self::Error> {
        match value {
            AnySource::Register1(r) => Ok(RegisterOrImmediate::Register1(r)),
            AnySource::Immediate1(r) => Ok(RegisterOrImmediate::Immediate1(r)),
            _ => Err(NotRegisterOrImmediate),
        }
    }
}

#[enum_dispatch(DestinationWriter)]
#[derive(Arbitrary)]
pub enum AnyDestination {
    Register1,
    AbsoluteStack,
    RelativeStack,
    AdvanceStackPointer,
}
