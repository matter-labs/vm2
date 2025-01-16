use std::mem;

use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use primitive_types::U256;
use sha3::digest::Digest;

use crate::heap::Heap;

pub(crate) const KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS: u16 = 0x8010;
const SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS: u16 = 0x02; // as in Ethereum
const ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS: u16 = 0x01; // as in Ethereum
const SECP256R1_VERIFY_PRECOMPILE_ADDRESS: u16 = 0x100; // As in RIP7212: https://github.com/ethereum/RIPs/blob/master/RIPS/rip-7212.md

/// Provides access to the input memory for a precompile call.
#[derive(Debug)]
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

#[derive(Debug, Default)]
pub struct PrecompileOutput {
    pub(crate) buffer: [U256; 2],
    pub(crate) len: u32,
}

impl From<U256> for PrecompileOutput {
    fn from(value: U256) -> Self {
        Self {
            buffer: [value, U256::zero()],
            len: 1,
        }
    }
}

impl From<[U256; 2]> for PrecompileOutput {
    fn from(value: [U256; 2]) -> Self {
        Self {
            buffer: value,
            len: 2,
        }
    }
}

/// Implements the `keccak256` precompile.
pub fn keccak256_precompile(memory: PrecompileMemoryReader<'_>) -> PrecompileOutput {
    const KECCAK_RATE_BYTES: usize = 136;

    let mut keccak = sha3::Keccak256::default();
    let mut buffer = [0_u8; KECCAK_RATE_BYTES];
    let mut next_idx = 0;
    for byte in memory {
        buffer[next_idx] = byte;
        next_idx = if next_idx == KECCAK_RATE_BYTES - 1 {
            keccak.update(buffer);
            buffer = [0_u8; KECCAK_RATE_BYTES];
            0
        } else {
            next_idx + 1
        };
    }

    if next_idx > 0 {
        keccak.update(&buffer[..next_idx]);
    }
    let digest = keccak.finalize();
    PrecompileOutput::from(U256::from_big_endian(&digest))
}

type Sha256InnerState = [u32; 8];

unsafe fn extract_sha256_state(core: sha2::Sha256VarCore) -> Sha256InnerState {
    // We use a trick that size of both structures is the same, and even though we do not know a stable field layout,
    // we can replicate it.
    struct MockSha256VarCore {
        state: Sha256InnerState,
        _block_len: u64,
    }

    let core: MockSha256VarCore = mem::transmute(core);
    core.state
}

pub fn sha256_precompile(
    mut memory: PrecompileMemoryReader<'_, true>,
    rounds: u64,
) -> PrecompileOutput {
    const SHA256_RATE_BYTES: usize = 64;

    if rounds == 0 {
        // Special case in the old VM
        return PrecompileOutput::default();
    }

    let mut sha256 = sha2::Sha256::default();
    let mut buffer = [0_u8; SHA256_RATE_BYTES];
    for _ in 0..rounds {
        for (dest, src) in buffer.iter_mut().zip(&mut memory) {
            *dest = src;
        }
        sha256.update(buffer);
    }
    let (core, _) = sha256.decompose();
    let sha256_state = unsafe {
        // SAFETY: this transmute is safe because `CtVariableCoreWrapper` is a thin wrapper around `Sha256VarCore`
        // (it only adds a zero-sized `PhantomData`).
        let stripped_core: sha2::Sha256VarCore = mem::transmute(core);
        // This should be safe provided `MockSha256VarCore` has the same layout as `Sha256VarCore`.
        extract_sha256_state(stripped_core)
    };

    // The old VM reverses endianness of `u32`s in `sha256_state`, so we do this as well.
    let mut sha256_state_bytes = [0_u8; 32];
    for (dest_bytes, state_u32) in sha256_state_bytes.chunks_mut(4).zip(sha256_state) {
        dest_bytes.copy_from_slice(&state_u32.to_be_bytes());
    }

    PrecompileOutput::from(U256::from_big_endian(&sha256_state_bytes))
}

// - hash of the message
// - v: 32 bytes; only the last byte is read
// - r: 32 bytes
// - s: 32 bytes
pub fn ecrecover_precompile(mut memory: PrecompileMemoryReader<'_, true>) -> PrecompileOutput {
    let mut digest = [0_u8; 32];
    for (dest_byte, src) in digest.iter_mut().zip(&mut memory) {
        *dest_byte = src;
    }

    for _ in 0..31 {
        memory.next(); // Skip 31 bytes of the `v`.
    }
    let v_byte = memory.next().unwrap_or(0);
    assert!(v_byte == 0 || v_byte == 1);
    let recovery_id = RecoveryId::from_byte(v_byte).unwrap(); // `unwrap()` is safe as checked above

    let mut signature_bytes = [0_u8; 64];
    for (dest_byte, src) in signature_bytes.iter_mut().zip(&mut memory) {
        *dest_byte = src;
    }

    if let Some(key) = ecrecover_inner(&digest, &signature_bytes, recovery_id) {
        let address_u256 = key_to_address(&key);
        PrecompileOutput::from([U256::one(), address_u256])
    } else {
        PrecompileOutput::from([U256::zero(), U256::zero()])
    }
}

fn ecrecover_inner(
    digest: &[u8; 32],
    signature_bytes: &[u8; 64],
    recovery_id: RecoveryId,
) -> Option<VerifyingKey> {
    let signature = Signature::from_slice(signature_bytes).ok()?;
    VerifyingKey::recover_from_prehash(digest, &signature, recovery_id).ok()
}

fn key_to_address(key: &VerifyingKey) -> U256 {
    let encoded_key = key.to_encoded_point(false);
    let encoded_key = &encoded_key.as_bytes()[1..];
    debug_assert_eq!(encoded_key.len(), 64);
    let address_digest = sha3::Keccak256::digest(encoded_key);
    let address_u256 = U256::from_big_endian(&address_digest);
    // Mask out upper bytes of the hash.
    address_u256 & U256::MAX >> (256 - 160)
}

pub fn secp256r1_verify_precompile(
    mut memory: PrecompileMemoryReader<'_, true>,
) -> PrecompileOutput {
    let mut digest = [0_u8; 32];
    for (dest_byte, src) in digest.iter_mut().zip(&mut memory) {
        *dest_byte = src;
    }

    let mut signature_bytes = [0_u8; 64];
    for (dest_byte, src) in signature_bytes.iter_mut().zip(&mut memory) {
        *dest_byte = src;
    }

    let mut key_bytes = [0_u8; 64];
    for (dest_byte, src) in key_bytes.iter_mut().zip(&mut memory) {
        *dest_byte = src;
    }

    if let Some(is_valid) = secp256r1_verify_inner(&digest, key_bytes, &signature_bytes) {
        let is_valid = if is_valid { U256::one() } else { U256::zero() };
        PrecompileOutput::from([U256::one(), is_valid])
    } else {
        PrecompileOutput::from([U256::zero(); 2])
    }
}

fn secp256r1_verify_inner(
    digest: &[u8; 32],
    key_bytes: [u8; 64],
    signature_bytes: &[u8; 64],
) -> Option<bool> {
    use p256::{
        ecdsa::{signature::hazmat::PrehashVerifier, Signature, VerifyingKey},
        EncodedPoint,
    };

    let vk_point = EncodedPoint::from_untagged_bytes(&key_bytes.into());
    let key = VerifyingKey::from_encoded_point(&vk_point).ok()?;
    let signature = Signature::from_slice(signature_bytes).ok()?;
    Some(key.verify_prehash(digest, &signature).is_ok())
}

pub fn default_call_precompile(
    address: u16,
    aux_data: u64,
    memory: PrecompileMemoryReader<'_>,
) -> PrecompileOutput {
    match address {
        KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => keccak256_precompile(memory),
        SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
            sha256_precompile(memory.assume_offset_in_words(), aux_data)
        }
        ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS => {
            ecrecover_precompile(memory.assume_offset_in_words())
        }
        SECP256R1_VERIFY_PRECOMPILE_ADDRESS => {
            secp256r1_verify_precompile(memory.assume_offset_in_words())
        }
        _ => {
            // A precompile call may be used just to burn gas
            PrecompileOutput::default()
        }
    }
}

#[allow(clippy::cast_possible_truncation)] // OK for tests
#[cfg(test)]
mod tests {
    use primitive_types::H160;
    use rand::Rng;
    use zk_evm_abstractions::{
        aux::Timestamp,
        precompiles::{
            ecrecover::ecrecover_function, keccak256::keccak256_rounds_function,
            secp256r1_verify::secp256r1_verify_function, sha256::sha256_rounds_function,
        },
        queries::LogQuery,
        vm::Memory,
    };
    use zkevm_opcode_defs::PrecompileCallABI;
    use zksync_vm2_interface::HeapId;

    use super::*;
    use crate::heap::Heaps;

    impl Memory for Heaps {
        fn execute_partial_query(
            &mut self,
            _monotonic_cycle_counter: u32,
            mut query: zk_evm_abstractions::queries::MemoryQuery,
        ) -> zk_evm_abstractions::queries::MemoryQuery {
            let page = HeapId::from_u32_unchecked(query.location.page.0);

            let start = query.location.index.0 * 32;
            if query.rw_flag {
                self.write_u256(page, start, query.value);
            } else {
                query.value = self[page].read_u256(start);
                query.value_is_pointer = false;
            }
            query
        }

        fn specialized_code_query(
            &mut self,
            _monotonic_cycle_counter: u32,
            _query: zk_evm_abstractions::queries::MemoryQuery,
        ) -> zk_evm_abstractions::queries::MemoryQuery {
            unimplemented!()
        }

        fn read_code_query(
            &self,
            _monotonic_cycle_counter: u32,
            _query: zk_evm_abstractions::queries::MemoryQuery,
        ) -> zk_evm_abstractions::queries::MemoryQuery {
            unimplemented!()
        }
    }

    fn create_query(input_offset: u32, input_len: u32, aux_data: u64) -> LogQuery {
        let abi = PrecompileCallABI {
            input_memory_offset: input_offset,
            input_memory_length: input_len,
            output_memory_offset: 0,
            output_memory_length: 2, // not read by implementations
            memory_page_to_read: HeapId::FIRST.as_u32(),
            memory_page_to_write: HeapId::FIRST_AUX.as_u32(),
            precompile_interpreted_data: aux_data,
        };
        LogQuery {
            timestamp: Timestamp(0),
            key: abi.to_u256(),
            // only two first fields are read by the precompile
            tx_number_in_block: Default::default(),
            aux_byte: Default::default(),
            shard_id: Default::default(),
            address: H160::default(),
            read_value: U256::default(),
            written_value: U256::default(),
            rw_flag: Default::default(),
            rollback: Default::default(),
            is_service: Default::default(),
        }
    }

    #[test]
    fn keccak_precompile_works() {
        let mut heaps = Heaps::new(&[]);
        let bytes: Vec<_> = (0_u32..2_048).map(|i| i as u8).collect();

        let initial_offset = 12;
        for (i, u256_chunk) in bytes.chunks(32).enumerate() {
            let offset = i as u32 * 32 + initial_offset;
            heaps.write_u256(HeapId::FIRST, offset, U256::from_big_endian(u256_chunk));
        }

        for input_len in 0..=bytes.len() as u32 {
            println!("testing input with length {input_len}");

            let query = create_query(initial_offset, input_len, 0);
            keccak256_rounds_function::<_, false>(0, query, &mut heaps);
            let old_output = heaps[HeapId::FIRST_AUX].read_u256(0);

            let new_output = keccak256_precompile(PrecompileMemoryReader::new(
                &heaps[HeapId::FIRST],
                initial_offset,
                input_len,
            ));
            assert_eq!(new_output.len, 1);
            let new_output = new_output.buffer[0];
            assert_eq!(new_output, old_output);
        }
    }

    #[test]
    fn sha256_precompile_works() {
        let mut heaps = Heaps::new(&[]);
        let bytes: Vec<_> = (0_u32..2_048).map(|i| i as u8).collect();

        let initial_offset_words = 12;
        for (i, u256_chunk) in bytes.chunks(32).enumerate() {
            let offset = i as u32 * 32 + initial_offset_words * 32;
            heaps.write_u256(HeapId::FIRST, offset, U256::from_big_endian(u256_chunk));
        }

        let max_round_count = bytes.len() as u32 / 64;
        for round_count in 0..=max_round_count {
            println!("testing input with {round_count} round(s)");

            // `sha256_rounds_function` interprets offset in words, rather than bytes
            let query = create_query(initial_offset_words, round_count * 2, round_count.into());
            sha256_rounds_function::<_, false>(0, query, &mut heaps);
            let old_output = heaps[HeapId::FIRST_AUX].read_u256(0);

            let new_output = sha256_precompile(
                PrecompileMemoryReader::new(
                    &heaps[HeapId::FIRST],
                    initial_offset_words,
                    round_count * 2,
                )
                .assume_offset_in_words(),
                round_count.into(),
            );
            assert!(new_output.len <= 1);
            let new_output = new_output.buffer[0];
            assert_eq!(new_output, old_output);
        }
    }

    #[derive(Debug)]
    enum Mutation {
        RecoveryId,
        Digest,
        R,
        S,
        Key,
    }

    #[test]
    fn ecrecover_precompile_works() {
        use k256::ecdsa::SigningKey;

        let mut rng = rand::thread_rng();
        let mut heaps = Heaps::new(&[]);
        let initial_offset_in_words = 3;
        let initial_offset = initial_offset_in_words * 32;

        for _ in 0..500 {
            let mutation = match rng.gen_range(0..10) {
                0 => Some(Mutation::RecoveryId),
                1 => Some(Mutation::Digest),
                2 => Some(Mutation::R),
                3 => Some(Mutation::S),
                _ => None,
            };

            let signing_key = SigningKey::random(&mut rng);
            let message = "test message!";
            let mut message_digest = sha3::Keccak256::digest(message);

            let (signature, recovery_id) = signing_key
                .sign_prehash_recoverable(&message_digest)
                .unwrap();
            if recovery_id.is_x_reduced() {
                continue;
            }
            let mut recovery_id = recovery_id.to_byte();

            println!(
                "testing key {:?} with mutation {mutation:?}",
                signing_key.verifying_key().to_encoded_point(true)
            );
            let mut signature_bytes = signature.to_bytes();

            match mutation {
                Some(Mutation::Digest) => {
                    message_digest[rng.gen_range(0..32)] ^= 1;
                }
                Some(Mutation::RecoveryId) => {
                    recovery_id = 1 - recovery_id;
                }
                Some(Mutation::R) => {
                    signature_bytes[rng.gen_range(0..32)] ^= 1;
                }
                Some(Mutation::S) => {
                    signature_bytes[rng.gen_range(32..64)] ^= 1;
                }
                _ => { /* Do nothing */ }
            }

            heaps.write_u256(
                HeapId::FIRST,
                initial_offset,
                U256::from_big_endian(&message_digest),
            );
            heaps.write_u256(HeapId::FIRST, initial_offset + 32, recovery_id.into());
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 64,
                U256::from_big_endian(&signature_bytes[..32]),
            );
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 96,
                U256::from_big_endian(&signature_bytes[32..]),
            );

            // `ecrecover_function` interprets offset as words
            let query = create_query(initial_offset_in_words, 4, 0);
            ecrecover_function::<_, false>(0, query, &mut heaps);

            let expected_address = key_to_address(signing_key.verifying_key());
            let is_success = heaps[HeapId::FIRST_AUX].read_u256(0);
            let address = heaps[HeapId::FIRST_AUX].read_u256(32);
            if mutation.is_some() {
                assert_ne!(address, expected_address);
            } else {
                assert_eq!(is_success, U256::one());
                assert_eq!(address, expected_address);
            }

            let new_output = ecrecover_precompile(
                PrecompileMemoryReader::new(&heaps[HeapId::FIRST], initial_offset_in_words, 4)
                    .assume_offset_in_words(),
            );
            assert_eq!(new_output.len, 2);
            assert_eq!(new_output.buffer[0], is_success);
            assert_eq!(new_output.buffer[1], address);
        }
    }

    #[test]
    fn secp256r1_precompile_works() {
        use p256::ecdsa::{signature::hazmat::PrehashSigner, Signature, SigningKey};

        let mut rng = rand::thread_rng();
        let mut heaps = Heaps::new(&[]);
        let initial_offset_in_words = 3;
        let initial_offset = initial_offset_in_words * 32;

        for _ in 0..500 {
            let mutation = match rng.gen_range(0..10) {
                0 => Some(Mutation::Key),
                1 => Some(Mutation::Digest),
                2 => Some(Mutation::R),
                3 => Some(Mutation::S),
                _ => None,
            };

            let signing_key = SigningKey::random(&mut rng);
            let message = "test message!";
            let mut message_digest = sha3::Keccak256::digest(message);

            let signature: Signature = signing_key.sign_prehash(&message_digest).unwrap();

            println!(
                "testing key {:?} with mutation {mutation:?}",
                signing_key.verifying_key().to_encoded_point(true)
            );
            let mut signature_bytes = signature.to_bytes();
            let mut key_bytes = signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..]
                .to_vec();
            assert_eq!(key_bytes.len(), 64);

            match mutation {
                Some(Mutation::Digest) => {
                    message_digest[rng.gen_range(0..32)] ^= 1;
                }
                Some(Mutation::R) => {
                    signature_bytes[rng.gen_range(0..32)] ^= 1;
                }
                Some(Mutation::S) => {
                    signature_bytes[rng.gen_range(32..64)] ^= 1;
                }
                Some(Mutation::Key) => {
                    key_bytes[rng.gen_range(0..64)] ^= 1;
                }
                _ => { /* Do nothing */ }
            }

            heaps.write_u256(
                HeapId::FIRST,
                initial_offset,
                U256::from_big_endian(&message_digest),
            );
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 32,
                U256::from_big_endian(&signature_bytes[..32]),
            );
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 64,
                U256::from_big_endian(&signature_bytes[32..]),
            );
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 96,
                U256::from_big_endian(&key_bytes[..32]),
            );
            heaps.write_u256(
                HeapId::FIRST,
                initial_offset + 128,
                U256::from_big_endian(&key_bytes[32..]),
            );

            // `secp256r1_verify_function` interprets offset as words
            let query = create_query(initial_offset_in_words, 5, 0);
            secp256r1_verify_function::<_, false>(0, query, &mut heaps);

            let is_ok = heaps[HeapId::FIRST_AUX].read_u256(0);
            let is_verified = heaps[HeapId::FIRST_AUX].read_u256(32);
            if mutation.is_none() {
                assert_eq!(is_ok, U256::one());
                assert_eq!(is_verified, U256::one());
            } else {
                assert!(is_ok.is_zero() || is_verified.is_zero());
            }

            let new_output = secp256r1_verify_precompile(
                PrecompileMemoryReader::new(&heaps[HeapId::FIRST], initial_offset_in_words, 5)
                    .assume_offset_in_words(),
            );
            assert_eq!(new_output.len, 2);
            assert_eq!(new_output.buffer[0], is_ok);
            assert_eq!(new_output.buffer[1], is_verified);
        }
    }
}
