//! Precompiles support.

use primitive_types::U256;
pub use zkevm_opcode_defs::system_params::{
    ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS, KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
    SECP256R1_VERIFY_PRECOMPILE_ADDRESS, SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
};
use zksync_vm2_interface::CycleStats;

pub use self::legacy::LegacyPrecompiles;
use crate::heap::Heap;

mod legacy;

/// Provides access to the input memory for a precompile call.
#[derive(Debug, Clone)]
pub struct PrecompileMemoryReader<'a, const IN_WORDS: bool = false> {
    heap: &'a Heap,
    offset: u32,
    len: u32,
}

impl<'a> PrecompileMemoryReader<'a> {
    pub(crate) fn new(heap: &'a Heap, offset: u32, len: u32) -> Self {
        Self { heap, offset, len }
    }

    /// Assumes that the input offset and length passed via ABI are measured in 32-byte words, rather than bytes.
    pub fn assume_offset_in_words(self) -> PrecompileMemoryReader<'a, true> {
        PrecompileMemoryReader {
            heap: self.heap,
            offset: self.offset * 32,
            len: self.len * 32,
        }
    }
}

/// Iterates over input bytes.
impl<const IN_WORDS: bool> Iterator for PrecompileMemoryReader<'_, IN_WORDS> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }

        // This assumes the offset never overflows
        let output = self.heap.read_byte(self.offset);
        self.offset += 1;
        self.len -= 1;
        Some(output)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len as usize, Some(self.len as usize))
    }
}

impl ExactSizeIterator for PrecompileMemoryReader<'_> {
    fn len(&self) -> usize {
        self.len as usize
    }
}

/// Output of a precompile call returned from [`Precompiles::call_precompile()`].
#[derive(Debug, Default)]
pub struct PrecompileOutput {
    pub(crate) buffer: [U256; 3],
    pub(crate) len: u32,
    pub(crate) cycle_stats: Option<CycleStats>,
}

impl PrecompileOutput {
    /// Assigns cycle stats for this output.
    #[must_use]
    pub fn with_cycle_stats(mut self, stats: CycleStats) -> Self {
        self.cycle_stats = Some(stats);
        self
    }
}

impl From<U256> for PrecompileOutput {
    fn from(value: U256) -> Self {
        Self {
            buffer: [value, U256::zero(), U256::zero()],
            len: 1,
            cycle_stats: None,
        }
    }
}

impl<const N: usize> From<[U256; N]> for PrecompileOutput
where
    [U256; N]: Default,
{
    fn from(value: [U256; N]) -> Self {
        let mut buffer = [U256::zero(); 3];
        buffer[..N].copy_from_slice(&value[..N]);

        Self {
            buffer,
            len: u32::try_from(N).expect("Not a valid length"),
            cycle_stats: None,
        }
    }
}

/// Encapsulates precompiles used during VM execution.
pub trait Precompiles {
    /// Calls to a precompile.
    fn call_precompile(
        &self,
        address_low: u16,
        memory: PrecompileMemoryReader<'_>,
        aux_input: u64,
    ) -> PrecompileOutput;
}
