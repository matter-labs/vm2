#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| vm2::run_arbitrary_program(data));
