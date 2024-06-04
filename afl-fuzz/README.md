# AFL++ based differential fuzzing

Compares behaviour of two VM implementations by running one instruction from an arbitrary start state.

Setup: `cargo install cargo-afl`

Use `sh fuzz.sh` (or customize the command to your liking) to start fuzzing.
`show_crash.sh` can be used to quickly run one of the found crashes and display all the necessary information
for fixing it.

The size of the search space is relatively small due to tricks explained in the single_instruction_test module.
`cargo run --bin check_input_size` prints out an estimate of the amount of information in the state in bytes.