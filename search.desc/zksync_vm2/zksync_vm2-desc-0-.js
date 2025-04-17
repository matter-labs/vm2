searchState.loadedDescShard("zksync_vm2", 0, "High-Performance ZKsync Era VM\nAlways execute the associated instruction.\nRepresents an empty storage slot.\nVM stop reason returned from <code>VirtualMachine::run()</code>.\nFat pointer to a heap location.\nExecute the associated instruction if the “equal” …\nExecute the associated instruction if either of “greater …\nExecute the associated instruction if the “greater than…\nExecute the associated instruction if either of “less …\nExecute the associated instruction if either of “less …\nExecute the associated instruction if the “less than” …\nExecute the associated instruction if the “equal” …\nSingle EraVM instruction (an opcode + <code>Arguments</code>).\nVM execution mode requirements (kernel only, not in static …\nThe executed program has panicked.\nPredicate for an instruction. Encoded so that comparing it …\nCompiled EraVM bytecode.\nThe executed program has finished and returned the …\nThe executed program has reverted returning the specified …\n<code>VirtualMachine</code> settings.\nOpaque snapshot of a <code>WorldDiff</code> output by its eponymous …\nOne of the tracers decided it is time to stop the VM.\nChange in a single storage slot.\nVM storage access operations.\nStorage slot information returned from …\nReturned when the bootloader writes to the heap location …\nHigh-performance out-of-circuit EraVM implementation.\nEncapsulates VM interaction with the external world. This …\nPending modifications to the global state that are …\nAddressing modes supported by EraVM.\nValue written to the slot.\nValue before the slot was written to.\nReturns a reference to the code page of this program.\nComputes the cost of writing a storage slot.\nLoads a bytecode with the specified hash.\nLoads bytecode bytes for the <code>decommit</code> opcode.\nReturns hashes of decommitted contract bytecodes in no …\nBytecode hash of the default account abstraction contract.\nReturns events emitted after the specified <code>snapshot</code> was …\nBytecode hash of the EVM interpreter.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCreates <code>Add</code> instruction with the provided params.\nCreates <code>And</code> instruction with the provided params.\nCreates an <code>AuxHeapRead</code> instruction with the provided …\nCreates an <code>AuxHeapWrite</code> instruction with the provided …\nCreates an <code>AuxMutating0</code> instruction with the provided …\nCreates a <code>Caller</code> instruction with the provided params.\nCreates a <code>CodeAddress</code> instruction with the provided params.\nCreates a <code>ContextMeta</code> instruction with the provided params.\nCreates an <code>SP</code> instruction with the provided params.\nCreates a <code>ContextU128</code> instruction with the provided params.\nCreates a <code>Decommit</code> instruction with the provided params.\nCreates <code>Div</code> instruction with the provided params.\nCreates an <code>ErgsLeft</code> instruction with the provided params.\nCreates an <code>Event</code> instruction with the provided params.\nCreates a <code>FarCall</code> instruction with the provided mode and …\nCreates a <code>HeapRead</code> instruction with the provided params.\nCreates a <code>HeapWrite</code> instruction with the provided params.\nCreates an <code>IncrementTxNumber</code> instruction with the provided …\nCreates a <em>invalid</em> instruction that will panic by draining …\nCreates a <code>Jump</code> instruction with the provided params.\nCreates an <code>L2ToL1Message</code> instruction with the provided …\nCreates <code>Mul</code> instruction with the provided params.\nCreates a <code>NearCall</code> instruction with the provided params.\nCreates a <code>Nop</code> instruction with the provided params.\nCreates <code>Or</code> instruction with the provided params.\nCreates a panic <code>Ret</code> instruction with the provided params.\nCreates a <code>PointerAdd</code> instruction with the provided params.\nCreates a <code>PointerPack</code> instruction with the provided params.\nCreates an <code>PointerRead</code> instruction with the provided …\nCreates a <code>PointerShrink</code> instruction with the provided …\nCreates a <code>PointerSub</code> instruction with the provided params.\nCreates a <code>PrecompileCall</code> instruction with the provided …\nCreates a normal <code>Ret</code> instruction with the provided params.\nCreates a revert <code>Ret</code> instruction with the provided params.\nCreates <code>RotateLeft</code> instruction with the provided params.\nCreates <code>RotateRight</code> instruction with the provided params.\nCreates a <code>SetContextU128</code> instruction with the provided …\nCreates <code>ShiftLeft</code> instruction with the provided params.\nCreates <code>ShiftRight</code> instruction with the provided params.\nCreates a <code>StorageRead</code> instruction with the provided params.\nCreates a <code>StorageWrite</code> instruction with the provided …\nCreates <code>Sub</code> instruction with the provided params.\nCreates a <code>This</code> instruction with the provided params.\nCreates a <code>TransientStorageRead</code> instruction with the …\nCreates a <code>TransientStorageWrite</code> instruction with the …\nCreates a new program from <code>U256</code> words.\nCreates <code>Xor</code> instruction with the provided params.\nGets changes for all touched storage slots.\nGets changes for storage slots touched after the specified …\nWriting to this address in the bootloader’s heap …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nConverts this pointer into a <code>U256</code> word.\nReturns if the storage slot is free both in terms of gas …\n<code>true</code> if the slot is not set in the <code>World</code>. A write may be …\nWhether a write to the slot would be considered an initial …\nReturns L2-to-L1 logs emitted after the specified <code>snapshot</code> …\nLength of the pointed slice in bytes.\nCreates a VM snapshot. The snapshot can then be rolled …\nID of the heap this points to.\nCreates a new program.\nCreates new requirements.\nCreates a new VM instance.\nCreates default requirements that always hold.\nAdditional pointer offset inside the …\nPops a previously made snapshot without rolling back to …\nPrecompiles support.\nReturns precompiles to be used.\nReturns recorded pubdata costs for all storage operations.\nReads the specified slot from the storage.\nSame as <code>Self::read_storage()</code>, but doesn’t request the …\nReturns how much of the extra gas limit is left and the …\nReturns the VM to the state it was in when …\nRuns this VM with the specified <code>World</code> and <code>Tracer</code> until an …\nGet a snapshot for selecting which logs &amp; co. to output …\n0-based index of the pointer start byte at the <code>memory</code> page.\nReturns recorded refunds for all storage operations.\nTest-only tools for EraVM.\nValue of the storage slot.\nProvides a reference to the <code>World</code> diff accumulated by VM …\nAbsolute addressing into stack.\nAbsolute stack addressing.\nAbsolute stack addressing.\nSame as <code>RelativeStack</code>, but moves the stack pointer on …\nRelative stack addressing that updates the stack pointer …\nRelative stack addressing that updates the stack pointer …\nAll supported addressing modes for the first destination …\nAll supported addressing modes for the first source …\nArguments provided to an instruction in an EraVM bytecode.\nAbsolute addressing into the code page of the currently …\nAddressing into the code page of the executing contract.\nImmediate value passed as a first instruction arg.\nImmediate mode.\nImmediate mode.\nImmediate value passed as a second instruction arg.\nError converting <code>AnySource</code> to <code>RegisterOrImmediate</code>.\nRepresentation of one of 16 VM registers.\nRegister passed as a first instruction argument.\nRegister mode.\nRegister mode.\nRegister mode.\nRegister passed as a second instruction argument.\nCombination of a register and an immediate value wrapped …\nRegister or immediate addressing modes required by some VM …\nRelative addressing into stack (relative to the VM stack …\nRelative stack addressing.\nRelative stack addressing.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nImmediate value.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreates arguments from the provided info.\nCreates a register with the specified 0-based index.\nRegister spec.\nPrecompiles implementation using legacy VM code.\nProvides access to the input memory for a precompile call.\nOutput of a precompile call returned from …\nEncapsulates precompiles used during VM execution.\nAssumes that the input offset and length passed via ABI …\nCalls to a precompile.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nAssigns cycle stats for this output.\nTest <code>World</code> implementation.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCreates a test world with the provided programs.")