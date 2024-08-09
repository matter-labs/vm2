//! # EraVM Stable Interface
//!
//! This crate defines an interface for tracers that will never change but may be extended.
//! To be precise, a tracer using this interface will work in any VM written against that
//! version or a newer one. Updating the tracer to depend on a newer interface version is
//! not necessary. In fact, tracers should depend on the oldest version that has the required
//! features.
//!
//! A struct implementing [Tracer] may read and mutate the VM's state via [StateInterface]
//! when particular opcodes are executed.
//!
//! ## Why is extreme backwards compatibility required here?
//!
//! Suppose VM1 uses stable interface version 1 and VM2 uses stable interface version 2.
//! With any sane design it would be trivial to take a tracer written for version 1 and
//! update it to work with version 2. However, then it can no longer be used with VM1.
//!
//! This exact thing caused us a lot of trouble when we put many versions of zk_evm in multivm.
//!
//! ## How do I add a new feature to the interface?
//!
//! Do not change the existing traits. In fact, you should delete existing code in the new
//! version that you publish and import it from the previous version instead.
//!
//! This is how you would add a new method to StateInterface and a new opcode.
//! ```
//! # trait StateInterface {}
//! # trait Tracer {
//! #     #[inline(always)]
//! #     fn before_old_opcode<S: StateInterface>(&mut self, _state: &mut S) {}
//! #     #[inline(always)]
//! #     fn after_old_opcode<S: StateInterface>(&mut self, _state: &mut S) {}
//! # }
//!
//! trait StateInterfaceV2: StateInterface {
//!     fn get_some_new_field(&self) -> u32;
//! }
//!
//! trait TracerV2 {
//!     // redefine all existing opcodes
//!     #[inline(always)]
//!     fn before_old_opcode<S: StateInterfaceV2>(&mut self, _state: &mut S) {}
//!     #[inline(always)]
//!     fn after_old_opcode<S: StateInterfaceV2>(&mut self, _state: &mut S) {}
//!
//!     #[inline(always)]
//!     fn before_new_opcode<S: StateInterfaceV2>(&mut self, _state: &mut S) {}
//!     #[inline(always)]
//!     fn after_new_opcode<S: StateInterfaceV2>(&mut self, _state: &mut S) {}
//! }
//!
//! impl<T: Tracer> TracerV2 for T {
//!     // repeat this for all existing opcodes
//!     fn before_old_opcode<S: StateInterfaceV2>(&mut self, state: &mut S) {
//!         <Self as Tracer>::before_old_opcode(self, state);
//!     }
//!     fn after_old_opcode<S: StateInterfaceV2>(&mut self, state: &mut S) {
//!         <Self as Tracer>::after_old_opcode(self, state);
//!     }
//! }
//!
//! // Now you can use the new features by implementing TracerV2
//! struct MyTracer;
//! impl TracerV2 for MyTracer {
//!     fn before_new_opcode<S: StateInterfaceV2>(&mut self, state: &mut S) {
//!         state.get_some_new_field();
//!     }
//! }
//! ```

mod state_interface;
mod tracer_interface;
pub use state_interface::*;
pub use tracer_interface::*;
