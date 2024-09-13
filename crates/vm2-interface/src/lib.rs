//! # EraVM Stable Interface
//!
//! This crate defines an interface for tracers that will never change but may be extended.
//! To be precise, a tracer using this interface will work in any VM written against that
//! version or a newer one. Updating the tracer to depend on a newer interface version is
//! not necessary. In fact, tracers should depend on the oldest version that has the required
//! features.
//!
//! A struct implementing [`Tracer`] may read and mutate the VM's state via [`StateInterface`]
//! when particular opcodes are executed.
//!
//! ## Why is extreme backwards compatibility required here?
//!
//! Suppose VM1 uses stable interface version 1 and VM2 uses stable interface version 2.
//! With any sane design it would be trivial to take a tracer written for version 1 and
//! update it to work with version 2. However, then it can no longer be used with VM1.
//!
//! This exact thing caused us a lot of trouble when we put many versions of `zk_evm` in `multivm`.
//!
//! ## How do I add a new feature to the interface?
//!
//! Do not change the existing traits. In fact, you should delete existing code in the new
//! version that you publish and import it from the previous version instead.
//!
//! This is how you would add a new method to [`StateInterface`] and a new opcode.
//!
//! ```
//! # use zksync_vm2_interface as zksync_vm2_interface_v1;
//! use zksync_vm2_interface_v1::{
//!     StateInterface as StateInterfaceV1, Tracer as TracerV1, opcodes::NearCall,
//! };
//!
//! trait StateInterface: StateInterfaceV1 {
//!     fn get_some_new_field(&self) -> u32;
//! }
//!
//! pub struct NewOpcode;
//!
//! #[derive(PartialEq, Eq)]
//! enum Opcode {
//!     NewOpcode,
//!     NearCall,
//!     // ...
//! }
//!
//! trait OpcodeType {
//!     const VALUE: Opcode;
//! }
//!
//! impl OpcodeType for NewOpcode {
//!     const VALUE: Opcode = Opcode::NewOpcode;
//! }
//!
//! // Do this for every old opcode
//! impl OpcodeType for NearCall {
//!     const VALUE: Opcode = Opcode::NearCall;
//! }
//!
//! trait Tracer {
//!     fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
//!     fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
//! }
//!
//! impl<T: TracerV1> Tracer for T {
//!     fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
//!         match OP::VALUE {
//!             Opcode::NewOpcode => {}
//!             // Do this for every old opcode
//!             Opcode::NearCall => {
//!                 <Self as TracerV1>::before_instruction::<NearCall, _>(self, state)
//!             }
//!         }
//!     }
//!     fn after_instruction<OP: OpcodeType, S: StateInterface>(&mut self, _state: &mut S) {}
//! }
//!
//! // Now you can use the new features by implementing TracerV2
//! struct MyTracer;
//! impl Tracer for MyTracer {
//!     fn before_instruction<OP: OpcodeType, S: StateInterface>(&mut self, state: &mut S) {
//!         if OP::VALUE == Opcode::NewOpcode {
//!             state.get_some_new_field();
//!         }
//!     }
//! }
//! ```

pub use self::{state_interface::*, tracer_interface::*};

mod state_interface;
mod tracer_interface;
