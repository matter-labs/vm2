use crate::{predication::Predicate, state::State};
use arbitrary::{Arbitrary, Unstructured};
use enum_dispatch::enum_dispatch;
use u256::U256;

pub(crate) trait Source {
    fn get(args: &Arguments, state: &mut State) -> U256;
}

pub(crate) trait Destination {
    fn set(args: &Arguments, state: &mut State, value: U256);
}

#[enum_dispatch]
pub(crate) trait SourceWriter {
    fn write_source(&self, args: &mut Arguments);
}

#[enum_dispatch]
pub trait DestinationWriter {
    fn write_destination(&self, args: &mut Arguments);
}

#[derive(Default)]
pub struct Arguments {
    source_registers: PackedRegisters,
    destination_registers: PackedRegisters,
    immediate1: u16,
    immediate2: u16,
    pub predicate: Predicate,
}

/// This one should only be used when [Register2] is used as well.
/// It must not be used simultaneously with Absolute/[RelativeStack].
#[derive(Arbitrary)]
pub struct Register1(pub Register);

#[derive(Arbitrary)]
pub struct Register2(pub Register);

impl Source for Register1 {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        args.source_registers.register1().get(state)
    }
}

impl SourceWriter for Register1 {
    fn write_source(&self, args: &mut Arguments) {
        args.source_registers.set_register1(self.0);
    }
}

impl Source for Register2 {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        args.source_registers.register2().get(state)
    }
}

impl SourceWriter for Register2 {
    fn write_source(&self, args: &mut Arguments) {
        args.source_registers.set_register2(self.0);
    }
}

impl Destination for Register1 {
    fn set(args: &Arguments, state: &mut State, value: U256) {
        args.destination_registers.register1().set(state, value);
    }
}

impl DestinationWriter for Register1 {
    fn write_destination(&self, args: &mut Arguments) {
        args.destination_registers.set_register1(self.0)
    }
}

impl Destination for Register2 {
    fn set(args: &Arguments, state: &mut State, value: U256) {
        args.destination_registers.register2().set(state, value);
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
    fn get(args: &Arguments, _state: &mut State) -> U256 {
        U256([args.immediate1 as u64, 0, 0, 0])
    }
}

impl SourceWriter for Immediate1 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate1 = self.0;
    }
}

impl Source for Immediate2 {
    fn get(args: &Arguments, _state: &mut State) -> U256 {
        U256([args.immediate2 as u64, 0, 0, 0])
    }
}

impl SourceWriter for Immediate2 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate2 = self.0;
    }
}

#[derive(Arbitrary, Clone)]
pub struct StackLikeParameters {
    pub immediate: u16,
    pub register: Register,
}

/// Any addressing mode that uses reg + imm in some way.
/// They all encode their parameters in the same way.
trait StackLike {
    fn inner(&self) -> &StackLikeParameters;
}

impl<T: StackLike> SourceWriter for T {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate1 = self.inner().immediate;
        args.source_registers.set_register1(self.inner().register);
    }
}

impl<T: StackLike> DestinationWriter for T {
    fn write_destination(&self, args: &mut Arguments) {
        args.immediate2 = self.inner().immediate;
        args.destination_registers
            .set_register1(self.inner().register)
    }
}

fn source_stack_address(args: &Arguments, state: &mut State) -> u16 {
    compute_stack_address(state, args.source_registers.register1(), args.immediate1)
}

pub fn destination_stack_address(args: &Arguments, state: &mut State) -> u16 {
    compute_stack_address(
        state,
        args.destination_registers.register1(),
        args.immediate2,
    )
}

/// Computes register + immediate (mod 2^16).
/// Stack addresses are always in that remainder class anyway.
fn compute_stack_address(state: &mut State, register: Register, immediate: u16) -> u16 {
    (register.get(state).low_u32() as u16).wrapping_add(immediate)
}

#[derive(Arbitrary)]
pub struct AbsoluteStack(pub StackLikeParameters);

impl StackLike for AbsoluteStack {
    fn inner(&self) -> &StackLikeParameters {
        &self.0
    }
}

impl Source for AbsoluteStack {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        state.stack[source_stack_address(args, state) as usize]
    }
}

impl Destination for AbsoluteStack {
    fn set(args: &Arguments, state: &mut State, value: U256) {
        state.stack[destination_stack_address(args, state) as usize] = value;
    }
}

#[derive(Arbitrary)]
pub struct RelativeStack(pub StackLikeParameters);

impl StackLike for RelativeStack {
    fn inner(&self) -> &StackLikeParameters {
        &self.0
    }
}

impl Source for RelativeStack {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        state.stack[state.sp.wrapping_sub(source_stack_address(args, state)) as usize]
    }
}

impl Destination for RelativeStack {
    fn set(args: &Arguments, state: &mut State, value: U256) {
        state.stack[state
            .sp
            .wrapping_add(destination_stack_address(args, state)) as usize] = value;
    }
}

#[derive(Arbitrary, Clone)]
pub struct AdvanceStackPointer(pub StackLikeParameters);

impl StackLike for AdvanceStackPointer {
    fn inner(&self) -> &StackLikeParameters {
        &self.0
    }
}

impl Source for AdvanceStackPointer {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        state.sp = state.sp.wrapping_sub(source_stack_address(args, state));
        state.stack[state.sp as usize]
    }
}

impl Destination for AdvanceStackPointer {
    fn set(args: &Arguments, state: &mut State, value: U256) {
        state.stack[state.sp as usize] = value;
        state.sp = state
            .sp
            .wrapping_add(destination_stack_address(args, state));
    }
}

#[derive(Arbitrary)]
pub struct CodePage(pub StackLikeParameters);

impl StackLike for CodePage {
    fn inner(&self) -> &StackLikeParameters {
        &self.0
    }
}

impl Source for CodePage {
    fn get(args: &Arguments, state: &mut State) -> U256 {
        let address = source_stack_address(args, state);
        state
            .code_page
            .get(address as usize)
            .cloned()
            .unwrap_or(U256::zero())
    }
}

#[derive(Copy, Clone)]
pub struct Register(u8);

impl Register {
    pub fn new(n: u8) -> Self {
        debug_assert!(n < 16);
        Self(n)
    }

    fn get(&self, state: &mut State) -> U256 {
        unsafe { *state.registers.get_unchecked(self.0 as usize) }
    }

    fn set(&self, state: &mut State, value: U256) {
        if self.0 != 0 {
            unsafe { *state.registers.get_unchecked_mut(self.0 as usize) = value };
        }
    }
}

impl<'a> Arbitrary<'a> for Register {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self, arbitrary::Error> {
        Ok(Register(u.choose_index(16)? as u8))
    }
}

#[derive(Default)]
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

#[enum_dispatch(DestinationWriter)]
#[derive(Arbitrary)]
pub enum AnyDestination {
    Register1,
    AbsoluteStack,
    RelativeStack,
    AdvanceStackPointer,
}
