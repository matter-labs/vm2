use primitive_types::{H160, U256};
use zk_evm_abstractions::{
    aux::Timestamp,
    precompiles::{
        ecrecover::ecrecover_function, keccak256::keccak256_rounds_function,
        secp256r1_verify::secp256r1_verify_function, sha256::sha256_rounds_function,
    },
    queries::{LogQuery, MemoryQuery},
    vm::Memory,
};
use zkevm_opcode_defs::{
    PrecompileCallABI, ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS,
    KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS, SECP256R1_VERIFY_PRECOMPILE_ADDRESS,
    SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
};
use zksync_vm2_interface::CycleStats;

use super::{PrecompileMemoryReader, PrecompileOutput, Precompiles};

fn create_query(input_offset: u32, input_len: u32, aux_data: u64) -> LogQuery {
    let abi = PrecompileCallABI {
        input_memory_offset: input_offset,
        input_memory_length: input_len,
        output_memory_offset: 0,
        output_memory_length: 2, // not read by implementations
        // Pages are fake; we assume that precompiles are implemented correctly and don't read / write anywhere but the specified pages
        memory_page_to_read: 1,
        memory_page_to_write: 2,
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

#[derive(Debug)]
struct LegacyIo<'a> {
    input: PrecompileMemoryReader<'a>,
    output: PrecompileOutput,
}

impl<'a> LegacyIo<'a> {
    fn new(input: PrecompileMemoryReader<'a>) -> Self {
        Self {
            input,
            output: PrecompileOutput::default(),
        }
    }
}

impl Memory for LegacyIo<'_> {
    fn execute_partial_query(
        &mut self,
        _monotonic_cycle_counter: u32,
        mut query: MemoryQuery,
    ) -> MemoryQuery {
        let start_word = query.location.index.0;
        if query.rw_flag {
            assert!(start_word < 2, "standard precompiles never write >2 words");
            self.output.buffer[start_word as usize] = query.value;
            self.output.len = self.output.len.max(start_word + 1);
        } else {
            // Access `Heap` directly for a speed-up
            query.value = self.input.heap.read_u256(start_word * 32);
            query.value_is_pointer = false;
        }
        query
    }

    fn specialized_code_query(
        &mut self,
        _monotonic_cycle_counter: u32,
        _query: MemoryQuery,
    ) -> MemoryQuery {
        unimplemented!("should not be called")
    }

    fn read_code_query(&self, _monotonic_cycle_counter: u32, _query: MemoryQuery) -> MemoryQuery {
        unimplemented!("should not be called")
    }
}

/// Precompiles implementation using legacy VM code.
#[derive(Debug)]
pub struct LegacyPrecompiles;

impl Precompiles for LegacyPrecompiles {
    #[allow(clippy::cast_possible_truncation)]
    fn call_precompile(
        &self,
        address_low: u16,
        memory: PrecompileMemoryReader<'_>,
        aux_input: u64,
    ) -> PrecompileOutput {
        let query = create_query(memory.offset, memory.len, aux_input);
        let mut io = LegacyIo::new(memory);
        match address_low {
            KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                let cycles = keccak256_rounds_function::<_, false>(0, query, &mut io).0;
                io.output
                    .with_cycle_stats(CycleStats::Keccak256(cycles as u32))
            }
            SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS => {
                let cycles = sha256_rounds_function::<_, false>(0, query, &mut io).0;
                io.output
                    .with_cycle_stats(CycleStats::Sha256(cycles as u32))
            }
            ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS => {
                let cycles = ecrecover_function::<_, false>(0, query, &mut io).0;
                io.output
                    .with_cycle_stats(CycleStats::EcRecover(cycles as u32))
            }
            SECP256R1_VERIFY_PRECOMPILE_ADDRESS => {
                let cycles = secp256r1_verify_function::<_, false>(0, query, &mut io).0;
                io.output
                    .with_cycle_stats(CycleStats::Secp256r1Verify(cycles as u32))
            }
            _ => PrecompileOutput::default(),
        }
    }
}

#[allow(clippy::cast_possible_truncation)] // OK for tests
#[cfg(test)]
mod tests {
    use proptest::{array, collection, num, option, prelude::*};
    use zkevm_opcode_defs::{
        k256::ecdsa::{SigningKey as K256SigningKey, VerifyingKey as K256VerifyingKey},
        p256::ecdsa::SigningKey as P256SigningKey,
        sha3::{self, Digest},
    };
    use zksync_vm2_interface::HeapId;

    use super::*;
    use crate::heap::Heaps;

    const MAX_LEN: usize = 2_048;

    fn arbitrary_aligned_bytes(alignment: usize) -> impl Strategy<Value = Vec<u8>> {
        (0..=(MAX_LEN / alignment)).prop_flat_map(move |len_in_words| {
            collection::vec(num::u8::ANY, len_in_words * alignment)
        })
    }

    fn key_to_address(key: &K256VerifyingKey) -> U256 {
        let encoded_key = key.to_encoded_point(false);
        let encoded_key = &encoded_key.as_bytes()[1..];
        debug_assert_eq!(encoded_key.len(), 64);
        let address_digest = sha3::Keccak256::digest(encoded_key);
        let address_u256 = U256::from_big_endian(&address_digest);
        // Mask out upper bytes of the hash.
        address_u256 & U256::MAX >> (256 - 160)
    }

    fn test_keccak_precompile(input: &[u8], initial_offset: u32) -> Result<(), TestCaseError> {
        let input_len = input.len() as u32;
        assert_eq!(input_len % 32, 0);

        let mut heaps = Heaps::new(&[]);
        for (i, u256_chunk) in input.chunks(32).enumerate() {
            let offset = i as u32 * 32 + initial_offset;
            heaps.write_u256(HeapId::FIRST, offset, U256::from_big_endian(u256_chunk));
        }

        let memory = PrecompileMemoryReader::new(&heaps[HeapId::FIRST], initial_offset, input_len);
        let output = LegacyPrecompiles.call_precompile(
            KECCAK256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
            memory,
            0,
        );

        prop_assert_eq!(output.len, 1);
        let expected_hash = sha3::Keccak256::digest(input);
        let expected_hash = U256::from_big_endian(&expected_hash);
        prop_assert_eq!(output.buffer[0], expected_hash);
        prop_assert!(matches!(output.cycle_stats, Some(CycleStats::Keccak256(_))));
        Ok(())
    }

    fn test_sha256_precompile(
        input: &[u8],
        initial_offset_in_words: u32,
    ) -> Result<(), TestCaseError> {
        assert_eq!(input.len() % 64, 0);
        let mut heaps = Heaps::new(&[]);

        for (i, u256_chunk) in input.chunks(32).enumerate() {
            let offset = i as u32 * 32 + initial_offset_in_words * 32;
            heaps.write_u256(HeapId::FIRST, offset, U256::from_big_endian(u256_chunk));
        }

        let max_round_count = input.len() as u32 / 64;
        for round_count in 0..=max_round_count {
            let memory = PrecompileMemoryReader::new(
                &heaps[HeapId::FIRST],
                initial_offset_in_words,
                round_count * 2,
            );
            let output = LegacyPrecompiles.call_precompile(
                SHA256_ROUND_FUNCTION_PRECOMPILE_ADDRESS,
                memory,
                round_count.into(),
            );

            if round_count == 0 {
                prop_assert_eq!(output.len, 0);
            } else {
                prop_assert_eq!(output.len, 1);
                prop_assert_ne!(output.buffer[0], U256::zero());
            }
            prop_assert!(matches!(output.cycle_stats, Some(CycleStats::Sha256(_))));
        }
        Ok(())
    }

    #[derive(Debug, Clone, Copy)]
    enum EcRecoverMutation {
        RecoveryId,
        Digest(usize),
        R(usize),
        S(usize),
    }

    impl EcRecoverMutation {
        fn gen() -> impl Strategy<Value = Self> {
            (0..4).prop_flat_map(|raw| match raw {
                0 => Just(Self::RecoveryId).boxed(),
                1 => (0_usize..32).prop_map(Self::Digest).boxed(),
                2 => (0_usize..32).prop_map(Self::R).boxed(),
                3 => (0_usize..32).prop_map(Self::S).boxed(),
                _ => unreachable!(),
            })
        }
    }

    fn test_ecrecover_precompile(
        signing_key: &K256SigningKey,
        mutation: Option<EcRecoverMutation>,
        initial_offset_in_words: u32,
    ) -> Result<(), TestCaseError> {
        let mut heaps = Heaps::new(&[]);
        let initial_offset = initial_offset_in_words * 32;

        let message = "test message!";
        let mut message_digest = sha3::Keccak256::digest(message);

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&message_digest)
            .unwrap();
        if recovery_id.is_x_reduced() {
            return Ok(());
        }
        let mut recovery_id = recovery_id.to_byte();

        println!(
            "testing key {:?} with mutation {mutation:?}",
            signing_key.verifying_key().to_encoded_point(true)
        );
        let mut signature_bytes = signature.to_bytes();

        match mutation {
            Some(EcRecoverMutation::Digest(byte)) => {
                message_digest[byte] ^= 1;
            }
            Some(EcRecoverMutation::RecoveryId) => {
                recovery_id = 1 - recovery_id;
            }
            Some(EcRecoverMutation::R(byte)) => {
                signature_bytes[byte] ^= 1;
            }
            Some(EcRecoverMutation::S(byte)) => {
                signature_bytes[byte + 32] ^= 1;
            }
            None => { /* Do nothing */ }
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

        let memory = PrecompileMemoryReader::new(&heaps[HeapId::FIRST], initial_offset_in_words, 4);
        let output = LegacyPrecompiles.call_precompile(
            ECRECOVER_INNER_FUNCTION_PRECOMPILE_ADDRESS,
            memory,
            0,
        );

        prop_assert_eq!(output.len, 2);
        let expected_address = key_to_address(signing_key.verifying_key());
        let [is_success, address] = output.buffer;
        if mutation.is_some() {
            prop_assert_ne!(address, expected_address);
        } else {
            prop_assert_eq!(is_success, U256::one());
            prop_assert_eq!(address, expected_address);
        }
        prop_assert!(matches!(output.cycle_stats, Some(CycleStats::EcRecover(1))));
        Ok(())
    }

    #[derive(Debug, Clone, Copy)]
    enum P256Mutation {
        Digest(usize),
        R(usize),
        S(usize),
        Key(usize),
    }

    impl P256Mutation {
        fn gen() -> impl Strategy<Value = Self> {
            (0..4).prop_flat_map(|raw| match raw {
                0 => (0_usize..32).prop_map(Self::Digest).boxed(),
                1 => (0_usize..32).prop_map(Self::R).boxed(),
                2 => (0_usize..32).prop_map(Self::S).boxed(),
                3 => (0_usize..64).prop_map(Self::Key).boxed(),
                _ => unreachable!(),
            })
        }
    }

    fn test_secp256r1_precompile(
        signing_key: &P256SigningKey,
        mutation: Option<P256Mutation>,
        initial_offset_in_words: u32,
    ) -> Result<(), TestCaseError> {
        use zkevm_opcode_defs::p256::ecdsa::{signature::hazmat::PrehashSigner, Signature};

        let mut heaps = Heaps::new(&[]);
        let initial_offset = initial_offset_in_words * 32;

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
            Some(P256Mutation::Digest(byte)) => {
                message_digest[byte] ^= 1;
            }
            Some(P256Mutation::R(byte)) => {
                signature_bytes[byte] ^= 1;
            }
            Some(P256Mutation::S(byte)) => {
                signature_bytes[byte + 32] ^= 1;
            }
            Some(P256Mutation::Key(byte)) => {
                key_bytes[byte] ^= 1;
            }
            None => { /* Do nothing */ }
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

        let memory = PrecompileMemoryReader::new(&heaps[HeapId::FIRST], initial_offset_in_words, 5);
        let output =
            LegacyPrecompiles.call_precompile(SECP256R1_VERIFY_PRECOMPILE_ADDRESS, memory, 0);

        prop_assert_eq!(output.len, 2);
        let [is_ok, is_verified] = output.buffer;
        if mutation.is_none() {
            prop_assert_eq!(is_ok, U256::one());
            prop_assert_eq!(is_verified, U256::one());
        } else {
            prop_assert!(is_ok.is_zero() || is_verified.is_zero());
        }
        prop_assert!(matches!(
            output.cycle_stats,
            Some(CycleStats::Secp256r1Verify(1))
        ));
        Ok(())
    }

    proptest! {
        #[test]
        fn keccak_precompile_works(
            bytes in arbitrary_aligned_bytes(32),
            initial_offset in 0..u32::MAX / 2,
        ) {
            test_keccak_precompile(&bytes, initial_offset)?;
        }

        #[test]
        fn sha256_precompile_works(
            bytes in arbitrary_aligned_bytes(64),
            initial_offset_in_words in 0..u32::MAX / 64,
        ) {
            test_sha256_precompile(&bytes, initial_offset_in_words)?;
        }

        #[test]
        fn ecrecover_precompile_works(
            signing_key in array::uniform32(num::u8::ANY)
                .prop_filter_map("not a key", |bytes| K256SigningKey::from_bytes(&bytes.into()).ok()),
            mutation in option::of(EcRecoverMutation::gen()),
            initial_offset_in_words in 0..u32::MAX / 64,
        ) {
            test_ecrecover_precompile(&signing_key, mutation, initial_offset_in_words)?;
        }

        #[test]
        fn secp256r1_precompile_works(
            signing_key in array::uniform32(num::u8::ANY)
                .prop_filter_map("not a key", |bytes| P256SigningKey::from_bytes(&bytes.into()).ok()),
            mutation in option::of(P256Mutation::gen()),
            initial_offset_in_words in 0..u32::MAX / 64,
        ) {
            test_secp256r1_precompile(&signing_key, mutation, initial_offset_in_words)?;
        }
    }
}
