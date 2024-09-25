# Changelog

## [0.2.1](https://github.com/matter-labs/vm2/compare/v0.2.0...v0.2.1) (2024-09-25)


### Bug Fixes

* some methods operating on wrong near call ([#69](https://github.com/matter-labs/vm2/issues/69)) ([9b36813](https://github.com/matter-labs/vm2/commit/9b36813ec6b4201396049f65b087c0bbf27d9ea2))

## [0.2.0](https://github.com/matter-labs/vm2/compare/v0.1.0...v0.2.0) (2024-09-23)


### Features

* Brush up repo for publishing ([#58](https://github.com/matter-labs/vm2/issues/58)) ([69ac5ed](https://github.com/matter-labs/vm2/commit/69ac5edd1b0ca7e0e38b6c2720dabb795526dbad))
* exposes necessary methods on heap newtype ([#43](https://github.com/matter-labs/vm2/issues/43)) ([9342db7](https://github.com/matter-labs/vm2/commit/9342db726462b76aa7e4ed246684b1316ea79c21))
* implement kernel mode ([#42](https://github.com/matter-labs/vm2/issues/42)) ([2407d39](https://github.com/matter-labs/vm2/commit/2407d39608e07e33b570f62d953bca04afb09e82))
* Stable tracer interface ([#46](https://github.com/matter-labs/vm2/issues/46)) ([dc73bb4](https://github.com/matter-labs/vm2/commit/dc73bb41f5ad103613c2c55a0e37d91ec2a9c338))
* Track `storage_refunds` and `pubdata_costs` stats ([#48](https://github.com/matter-labs/vm2/issues/48)) ([2882a12](https://github.com/matter-labs/vm2/commit/2882a1232a695ffc1ec4b796195f7aababeb6ab2))


### Bug Fixes

* base being in the kernel on address, not code address ([#31](https://github.com/matter-labs/vm2/issues/31)) ([d9cb911](https://github.com/matter-labs/vm2/commit/d9cb9114f26c10edf3b358a3a2c140214e1db5d8))
* bugs in initial writes change ([#36](https://github.com/matter-labs/vm2/issues/36)) ([8defb4a](https://github.com/matter-labs/vm2/commit/8defb4ad9643b87151e00030166f90763bcf356d))
* don't repeatedly get initial values ([#35](https://github.com/matter-labs/vm2/issues/35)) ([50fdbfa](https://github.com/matter-labs/vm2/commit/50fdbfad7723e0a7b91639cb64a40ae46a6d40f6))
* filter out initial writes of zero ([#39](https://github.com/matter-labs/vm2/issues/39)) ([a291c24](https://github.com/matter-labs/vm2/commit/a291c246bbd8fc2620b6ac61c0d9535b00c6bde5))
* Fix `Heap` equality comparison ([#51](https://github.com/matter-labs/vm2/issues/51)) ([a0cf04b](https://github.com/matter-labs/vm2/commit/a0cf04b03ac1c486a48e5f2e32422a00c27a1b9d))
* Fix decommit opcode semantics ([2882a12](https://github.com/matter-labs/vm2/commit/2882a1232a695ffc1ec4b796195f7aababeb6ab2))
* Fix decommitment cost divergence ([#57](https://github.com/matter-labs/vm2/issues/57)) ([d385127](https://github.com/matter-labs/vm2/commit/d385127d8715050cdc5c1265df3f80e98c7a73f4))
* Fix decommitment logic on out-of-gas ([#56](https://github.com/matter-labs/vm2/issues/56)) ([2276b7b](https://github.com/matter-labs/vm2/commit/2276b7b5af520fca0477bdafe43781b51896d235))
* fuzz test ([#30](https://github.com/matter-labs/vm2/issues/30)) ([d516967](https://github.com/matter-labs/vm2/commit/d5169679cf880eb5cebdf653319557ce19c97446))
* fuzz.sh ([#64](https://github.com/matter-labs/vm2/issues/64)) ([e8e72b5](https://github.com/matter-labs/vm2/commit/e8e72b5db786bf3bb55688ed5ef7ea4bf27a19f6))
* fuzzer now makes short programs; fix crash in near call ([#52](https://github.com/matter-labs/vm2/issues/52)) ([985a778](https://github.com/matter-labs/vm2/commit/985a778e029a8574150c1d526aa75109b5844444))
* infinite test ([#34](https://github.com/matter-labs/vm2/issues/34)) ([81185a5](https://github.com/matter-labs/vm2/commit/81185a545635f9bd23d05878b56049baea20903b))
* invalid instruction unsoundness ([#61](https://github.com/matter-labs/vm2/issues/61)) ([74577d9](https://github.com/matter-labs/vm2/commit/74577d9be13b1bff9d1a712389731f669b179e47))
* record history for aux heap as well ([#49](https://github.com/matter-labs/vm2/issues/49)) ([2877059](https://github.com/matter-labs/vm2/commit/28770597a3f150dbe4373cb57929bd8db82e884f))
* record pubdata used by precompiles ([#27](https://github.com/matter-labs/vm2/issues/27)) ([a7de066](https://github.com/matter-labs/vm2/commit/a7de066a212dc4d547464b62016debe0994aba30))
* report correct heap sizes in ContextMeta ([#26](https://github.com/matter-labs/vm2/issues/26)) ([493fcec](https://github.com/matter-labs/vm2/commit/493fcec74855bcf9b6ab91b1a3da077b2982f739))
* revert pubdata on failed near call, too ([#28](https://github.com/matter-labs/vm2/issues/28)) ([fabc553](https://github.com/matter-labs/vm2/commit/fabc553d6a7a13b58465745ad1554dcd8d9ec1a0))
* StateInterface::current_frame did not work with near calls ([#65](https://github.com/matter-labs/vm2/issues/65)) ([53f8f88](https://github.com/matter-labs/vm2/commit/53f8f88c0861fb1cefa002a10937e3e2952a90d2))
* track transaction number in changes ([#33](https://github.com/matter-labs/vm2/issues/33)) ([e683ae8](https://github.com/matter-labs/vm2/commit/e683ae8e600bfae85415edbdfcea0b727f462f4c))


### Performance Improvements

* Implement segmented heap ([#53](https://github.com/matter-labs/vm2/issues/53)) ([d2405bc](https://github.com/matter-labs/vm2/commit/d2405bc84d375c3b5e7bbade7e5045bf5e91a0d9))
* optimize external snapshots ([#47](https://github.com/matter-labs/vm2/issues/47)) ([952ecd4](https://github.com/matter-labs/vm2/commit/952ecd419081d433ad609663752ce546ad6cc4e1))
