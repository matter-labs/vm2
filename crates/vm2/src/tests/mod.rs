//! Low-level VM tests.

mod abort_unwind;
mod bytecode_behaviour;
mod divergence_regressions;
mod far_call_decommitment;
mod frame_stipend_counter;
mod memory_ceiling;
mod panic;
mod trace_failing_far_call;
