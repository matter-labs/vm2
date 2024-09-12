# High-Performance ZKsync Era VM (EraVM)

A high-performance rewrite of the out-of-circuit VM for ZKsync Era (aka EraVM).

See [Era docs](https://github.com/matter-labs/zksync-era/tree/main/docs/specs/zk_evm) for the VM overview and formal specification.

## Overview

This repository contains the following crates:

- [`zksync_vm2_interface`](crates/vm2-interface): stable VM interface for tracers
- [`zksync_vm2`](crates/vm2): VM implementation itself
- [`zksync_vm2_afl_fuzz`](tests/afl-fuzz): [AFL](https://crates.io/crates/afl)-based fuzzing for the VM.

## Policies

- [Security policy](SECURITY.md)
- [Contribution policy](CONTRIBUTING.md)

## License

ZKsync Era VM is distributed under the terms of either

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/blog/license/mit/>)

at your option.
