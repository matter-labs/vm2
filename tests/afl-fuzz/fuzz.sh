export AFL_AUTORESUME=1
cargo afl build --release && cargo afl fuzz -i in -o out -g $(cargo run --bin check_input_size) ../../target/release/zksync_vm2_afl_fuzz
