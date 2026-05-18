# AFL++ based differential fuzzing

Compares behaviour of two VM implementations by running one instruction from an arbitrary start state.
Finds divergences and instructions that put vm2 in an invalid state.

Setup: `cargo install cargo-afl`

Use `sh fuzz.sh` (or customize the command to your liking) to start fuzzing.
`show_crash.sh` can be used to quickly run one of the found crashes and display all the necessary information
for fixing it.

The size of the search space is relatively small due to tricks explained in the single_instruction_test module.
`cargo run --bin check_input_size` prints out an estimate of the amount of information in the state in bytes.

## Structured divergence validator

`vm2-divergence-validator` exposes the same single-instruction differential check as a JSON-emitting
CLI. It is useful for validating audit findings without writing a Rust regression test first.

```sh
cargo run --bin vm2-divergence-validator -- finding.yaml
cargo run --bin vm2-divergence-validator -- out/default/crashes/<testcase>
```

Exit codes:

| Code | Meaning |
|------|---------|
| 0 | Match |
| 1 | Divergence found |
| 2 | Error or testcase outside current harness coverage |

Minimal finding scenario:

```yaml
instruction: "0x0000000000000000"
frame:
  gas: 1000000
registers:
  r1:
    value: "0x00"
    pointer: false
flags:
  less_than: false
  equal: false
  greater_than: false
stack:
  read_value: "0x00"
  read_pointer: false
memory:
  heap_read_u256: "0x00"
storage_read: "0x00"
```

Register values may be plain scalars, tagged values, or fat pointers:

```yaml
registers:
  r2: "0x1234"
  r3:
    value: "0x1234"
    pointer: false
  r4:
    memory_page: 2050
    start: 0
    offset: 0
    length: 32
```

This validator accepts YAML / JSON finding scenarios and still accepts the AFL/arbitrary byte format
for crash triage. It is intentionally narrower than a full transaction-level scenario runner; use
`eravm-airbender-verifier/crates/vm_compare` for batch-level legacy-vs-FastVM comparison.

For findings that need more than one instruction, provide a `program` array and set `cycles` to the
number of VM cycles to execute. Single-instruction scenarios can use the top-level `instruction`
field instead.
