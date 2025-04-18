//! Addressing modes supported by EraVM.

#[cfg(feature = "arbitrary")]
use arbitrary::{Arbitrary, Unstructured};
use enum_dispatch::enum_dispatch;
use primitive_types::U256;
use zkevm_opcode_defs::erase_fat_pointer_metadata;

use crate::{mode_requirements::ModeRequirements, predication::Predicate};

pub(crate) trait Source {
    /// Get a word's value for non-pointer operations. (Pointers are erased.)
    fn get(args: &Arguments, state: &mut impl Addressable) -> U256 {
        Self::get_with_pointer_flag_and_erasing(args, state).0
    }

    /// Get a word's value and pointer flag.
    fn get_with_pointer_flag(args: &Arguments, state: &mut impl Addressable) -> (U256, bool) {
        (Self::get(args, state), false)
    }

    /// Get a word's value, erasing pointers but also returning the pointer flag.
    /// The flag will always be false unless in kernel mode.
    /// Necessary for pointer operations, which for some reason erase their second argument
    /// but also panic when it was a pointer.
    fn get_with_pointer_flag_and_erasing(
        args: &Arguments,
        state: &mut impl Addressable,
    ) -> (U256, bool) {
        let (mut value, is_pointer) = Self::get_with_pointer_flag(args, state);
        if is_pointer && !state.in_kernel_mode() {
            erase_fat_pointer_metadata(&mut value);
        }
        (value, is_pointer && state.in_kernel_mode())
    }
}

pub(crate) trait Destination {
    /// Set this register/stack location to value and clear its pointer flag
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256);

    /// Same as `set` but sets the pointer flag
    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256);
}

/// The part of VM state that addressing modes need to operate on
pub(crate) trait Addressable {
    fn registers(&mut self) -> &mut [U256; 16];
    fn register_pointer_flags(&mut self) -> &mut u16;

    fn read_stack(&mut self, slot: u16) -> U256;
    fn write_stack(&mut self, slot: u16, value: U256);
    fn stack_pointer(&mut self) -> &mut u16;

    fn read_stack_pointer_flag(&mut self, slot: u16) -> bool;
    fn set_stack_pointer_flag(&mut self, slot: u16);
    fn clear_stack_pointer_flag(&mut self, slot: u16);

    fn code_page(&self) -> &[U256];

    fn in_kernel_mode(&self) -> bool;
}

#[enum_dispatch]
pub(crate) trait SourceWriter {
    fn write_source(&self, args: &mut Arguments);
}

impl<T: SourceWriter> SourceWriter for Option<T> {
    fn write_source(&self, args: &mut Arguments) {
        if let Some(x) = self {
            x.write_source(args);
        }
    }
}

#[enum_dispatch]
pub(crate) trait DestinationWriter {
    fn write_destination(&self, args: &mut Arguments);
}

impl<T: DestinationWriter> DestinationWriter for Option<T> {
    fn write_destination(&self, args: &mut Arguments) {
        if let Some(x) = self {
            x.write_destination(args);
        }
    }
}

/// Arguments provided to an instruction in an EraVM bytecode.
// It is important for performance that this fits into 8 bytes.
#[derive(Debug)]
pub struct Arguments {
    source_registers: PackedRegisters,
    destination_registers: PackedRegisters,
    immediate1: u16,
    immediate2: u16,
    predicate_and_mode_requirements: u8,
    static_gas_cost: u8,
}

pub(crate) const L1_MESSAGE_COST: u32 = 156_250;
pub(crate) const SSTORE_COST: u32 = 5_511;
pub(crate) const SLOAD_COST: u32 = 2_008;
pub(crate) const INVALID_INSTRUCTION_COST: u32 = 4_294_967_295;

impl Arguments {
    /// Creates arguments from the provided info.
    #[allow(clippy::missing_panics_doc)] // never panics on properly created inputs
    pub const fn new(
        predicate: Predicate,
        gas_cost: u32,
        mode_requirements: ModeRequirements,
    ) -> Self {
        // Make sure that these two can be packed into 8 bits without overlapping
        assert!(predicate as u8 & (0b11 << 6) == 0);
        assert!(mode_requirements.0 & !0b11 == 0);

        Self {
            source_registers: PackedRegisters(0),
            destination_registers: PackedRegisters(0),
            immediate1: 0,
            immediate2: 0,
            predicate_and_mode_requirements: ((predicate as u8) << 2) | mode_requirements.0,
            static_gas_cost: Self::encode_static_gas_cost(gas_cost),
        }
    }

    #[allow(clippy::cast_possible_truncation)] // checked
    const fn encode_static_gas_cost(x: u32) -> u8 {
        match x {
            L1_MESSAGE_COST => 1,
            SSTORE_COST => 2,
            SLOAD_COST => 3,
            INVALID_INSTRUCTION_COST => 4,
            1..=4 => panic!("Reserved gas cost values overlap with actual gas costs"),
            x => {
                if x > u8::MAX as u32 {
                    panic!("Gas cost doesn't fit into 8 bits");
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

    pub(crate) fn predicate(&self) -> Predicate {
        unsafe { std::mem::transmute(self.predicate_and_mode_requirements >> 2) }
    }

    pub(crate) fn mode_requirements(&self) -> ModeRequirements {
        ModeRequirements(self.predicate_and_mode_requirements & 0b11)
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

/// Register passed as a first instruction argument.
///
/// It must not be used simultaneously with [`AbsoluteStack`], [`RelativeStack`], [`AdvanceStackPointer`],
/// or [`CodePage`].
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Register1(pub Register);

/// Register passed as a second instruction argument.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Register2(pub Register);

impl Source for Register1 {
    fn get_with_pointer_flag(args: &Arguments, state: &mut impl Addressable) -> (U256, bool) {
        let register = args.source_registers.register1();
        (register.value(state), register.pointer_flag(state))
    }
}

impl SourceWriter for Register1 {
    fn write_source(&self, args: &mut Arguments) {
        args.source_registers.set_register1(self.0);
    }
}

impl Source for Register2 {
    fn get_with_pointer_flag(args: &Arguments, state: &mut impl Addressable) -> (U256, bool) {
        let register = args.source_registers.register2();
        (register.value(state), register.pointer_flag(state))
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
        args.destination_registers.set_register1(self.0);
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
        args.destination_registers.set_register2(self.0);
    }
}

/// Immediate value passed as a first instruction arg.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Immediate1(pub u16);

/// Immediate value passed as a second instruction arg.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Immediate2(pub u16);

impl Immediate1 {
    pub(crate) fn get_u16(args: &Arguments) -> u16 {
        args.immediate1
    }
}

impl Immediate2 {
    pub(crate) fn get_u16(args: &Arguments) -> u16 {
        args.immediate2
    }
}

impl Source for Immediate1 {
    fn get(args: &Arguments, _state: &mut impl Addressable) -> U256 {
        U256([args.immediate1.into(), 0, 0, 0])
    }
}

impl SourceWriter for Immediate1 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate1 = self.0;
    }
}

impl Source for Immediate2 {
    fn get(args: &Arguments, _state: &mut impl Addressable) -> U256 {
        U256([args.immediate2.into(), 0, 0, 0])
    }
}

impl SourceWriter for Immediate2 {
    fn write_source(&self, args: &mut Arguments) {
        args.immediate2 = self.0;
    }
}

/// Combination of a register and an immediate value wrapped by [`AbsoluteStack`], [`RelativeStack`],
/// [`AdvanceStackPointer`] and [`CodePage`] addressing modes.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct RegisterAndImmediate {
    /// Immediate value.
    pub immediate: u16,
    /// Register spec.
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
            .set_register1(self.inner().register);
    }
}

trait StackAddressing {
    fn address_for_get(args: &Arguments, state: &mut impl Addressable) -> u16;
    fn address_for_set(args: &Arguments, state: &mut impl Addressable) -> u16;
}

impl<T: StackAddressing> Source for T {
    fn get_with_pointer_flag(args: &Arguments, state: &mut impl Addressable) -> (U256, bool) {
        let address = Self::address_for_get(args, state);
        (
            state.read_stack(address),
            state.read_stack_pointer_flag(address),
        )
    }
}

impl<T: StackAddressing> Destination for T {
    fn set(args: &Arguments, state: &mut impl Addressable, value: U256) {
        let address = Self::address_for_set(args, state);
        state.write_stack(address, value);
        state.clear_stack_pointer_flag(address);
    }

    fn set_fat_ptr(args: &Arguments, state: &mut impl Addressable, value: U256) {
        let address = Self::address_for_set(args, state);
        state.write_stack(address, value);
        state.set_stack_pointer_flag(address);
    }
}

fn source_stack_address(args: &Arguments, state: &mut impl Addressable) -> u16 {
    compute_stack_address(state, args.source_registers.register1(), args.immediate1)
}

pub(crate) fn destination_stack_address(args: &Arguments, state: &mut impl Addressable) -> u16 {
    compute_stack_address(
        state,
        args.destination_registers.register1(),
        args.immediate2,
    )
}

/// Computes register + immediate (mod 2^16).
/// Stack addresses are always in that remainder class anyway.
#[allow(clippy::cast_possible_truncation)]
fn compute_stack_address(state: &mut impl Addressable, register: Register, immediate: u16) -> u16 {
    (register.value(state).low_u32() as u16).wrapping_add(immediate)
}

/// Absolute addressing into stack.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
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

/// Relative addressing into stack (relative to the VM stack pointer).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
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

/// Same as [`RelativeStack`], but moves the stack pointer on access (decreases it when reading data;
/// increases when writing data).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
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

/// Absolute addressing into the code page of the currently executing program.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
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
            .copied()
            .unwrap_or(U256::zero())
    }
}

/// Representation of one of 16 VM registers.
#[derive(Debug, Clone, Copy)]
pub struct Register(u8);

impl Register {
    /// Creates a register with the specified 0-based index.
    ///
    /// # Panics
    ///
    /// Panics if `n >= 16`; EraVM has 16 registers.
    pub const fn new(n: u8) -> Self {
        assert!(n < 16, "EraVM has 16 registers");
        Self(n)
    }

    fn value(self, state: &mut impl Addressable) -> U256 {
        unsafe { *state.registers().get_unchecked(self.0 as usize) }
    }

    fn pointer_flag(self, state: &mut impl Addressable) -> bool {
        *state.register_pointer_flags() & (1 << self.0) != 0
    }

    fn set(self, state: &mut impl Addressable, value: U256) {
        if self.0 != 0 {
            unsafe { *state.registers().get_unchecked_mut(self.0 as usize) = value };
            *state.register_pointer_flags() &= !(1 << self.0);
        }
    }

    fn set_ptr(self, state: &mut impl Addressable, value: U256) {
        if self.0 != 0 {
            unsafe { *state.registers().get_unchecked_mut(self.0 as usize) = value };
            *state.register_pointer_flags() |= 1 << self.0;
        }
    }
}

#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for Register {
    #[allow(clippy::cast_possible_truncation)] // false positive: the value is <16
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self, arbitrary::Error> {
        Ok(Register(u.choose_index(16)? as u8))
    }
}

#[derive(Hash, Debug)]
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

/// All supported addressing modes for the first source argument.
#[enum_dispatch(SourceWriter)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub enum AnySource {
    /// Register mode.
    Register1,
    /// Immediate mode.
    Immediate1,
    /// Absolute stack addressing.
    AbsoluteStack,
    /// Relative stack addressing.
    RelativeStack,
    /// Relative stack addressing that updates the stack pointer on access.
    AdvanceStackPointer,
    /// Addressing into the code page of the executing contract.
    CodePage,
}

/// Register or immediate addressing modes required by some VM instructions.
#[enum_dispatch(SourceWriter)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub enum RegisterOrImmediate {
    /// Register mode.
    Register1,
    /// Immediate mode.
    Immediate1,
}

/// Error converting [`AnySource`] to [`RegisterOrImmediate`].
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

/// All supported addressing modes for the first destination argument.
#[enum_dispatch(DestinationWriter)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub enum AnyDestination {
    /// Register mode.
    Register1,
    /// Absolute stack addressing.
    AbsoluteStack,
    /// Relative stack addressing.
    RelativeStack,
    /// Relative stack addressing that updates the stack pointer on access.
    AdvanceStackPointer,
}
