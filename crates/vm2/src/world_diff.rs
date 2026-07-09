use std::collections::BTreeMap;

use primitive_types::{H160, U256};
use zk_evm_abstractions::{aux::Timestamp, queries::LogQuery};
use zkevm_opcode_defs::system_params::{
    STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST, STORAGE_ACCESS_WARM_READ_COST,
    STORAGE_ACCESS_WARM_WRITE_COST, STORAGE_AUX_BYTE,
};
use zksync_vm2_interface::{CycleStats, Event, HeapId, L2ToL1Log, Tracer};

use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap, RollbackablePod, RollbackableSet},
    StorageInterface, StorageSlot,
};

/// Merged value for `storage_writes`: pending written value + pubdata paid
/// (formerly the separate `storage_changes` + `paid_changes` maps).
#[derive(Debug, Clone, Copy, Default)]
pub struct StorageWriteEntry {
    /// Pending written value for the slot.
    pub value: U256,
    /// Pubdata cost paid for writing this slot (0 for free-storage slots).
    pub paid: u32,
}

/// Per-slot access flags packed into `WorldDiff::slot_flags` (one byte per
/// `(address, key)`), replacing three separate `(address, key)` sets.
const SLOT_READ: u8 = 1;
const SLOT_WRITTEN: u8 = 1 << 1;
/// Set on a read at rollback-depth zero (`did_read_at_depth_zero` in
/// `circuit_sequencer_api::sort_storage_access`). Downstream this forces a
/// *protective read* into the deduplicated storage set — unless the slot is
/// also written, in which case the write entry subsumes it.
const SLOT_PROTECTIVE_READ: u8 = 1 << 2;

/// Pending modifications to the global state that are executed at the end of a block.
/// In other words, side effects.
#[derive(Debug, Default)]
pub struct WorldDiff {
    // These are rolled back on revert or panic (and when the whole VM is rolled back).
    /// Pending storage writes (value + pubdata paid), merged from the former
    /// `storage_changes` + `paid_changes` to store the (address, key) once.
    storage_writes: RollbackableMap<(H160, U256), StorageWriteEntry>,
    transient_storage_changes: RollbackableMap<(H160, U256), U256>,
    events: RollbackableLog<Event>,
    l2_to_l1_logs: RollbackableLog<L2ToL1Log>,
    pub(crate) pubdata: RollbackablePod<i32>,
    storage_refunds: RollbackableLog<u32>,
    pubdata_costs: RollbackableLog<i32>,
    storage_logs: Vec<LogQuery>,
    rollback_storage_logs: Vec<LogQuery>,
    // The fields below are only rolled back when the whole VM is rolled back.
    /// Tracks decommit visibility state for each bytecode hash.
    ///
    /// Besides successful decommits, we also retain far-call decommit attempts that failed with
    /// out-of-gas in `pay_for_decommit()`. Legacy VM includes those hashes into "used contracts"
    /// output, and shadow-mode compares that output (`CurrentExecutionState.used_contract_hashes`).
    ///
    /// This field is rolled back only by external VM snapshots.
    pub(crate) decommitted_hashes: RollbackableMap<U256, DecommitState>,
    /// Reverse index for `decommitted_hashes` entries that carry materialized heap pages.
    ///
    /// This is used to quickly check whether a heap page is globally pinned by decommitment reuse
    /// semantics.
    ///
    /// This follows external snapshot / rollback semantics together with `decommitted_hashes`.
    decommit_pinned_pages: RollbackableSet<u32>,
    /// Per-slot access flags merged from three former `(address, key)` sets
    /// (read / written / protective-read) so the 52-byte key is stored once
    /// instead of three times. External-rollback semantics (whole-VM only),
    /// matching the original sets. See the `SLOT_*` consts.
    ///
    /// `SLOT_PROTECTIVE_READ` keeps the dedup's `did_read_at_depth_zero`
    /// predicate: set only by `read_storage_inner`, only in opt-out mode, and
    /// only when `storage_writes` has no pending write for the slot at read time.
    slot_flags: RollbackableMap<(H160, U256), u8>,

    // This is never rolled back. It is just a cache to avoid asking these from DB every time.
    storage_initial_values: BTreeMap<(H160, U256), StorageSlot>,

    /// Selects two mutually exclusive bookkeeping modes (see
    /// [`Self::set_record_storage_logs`] for the rationale); set once before
    /// execution, never rolled back.
    ///
    /// - `false` (default): append the `storage_logs` / `rollback_storage_logs`
    ///   trace; otherwise behave like the pre-optimization base — read-only slots
    ///   are *not* cached in `storage_initial_values`.
    /// - `true`: drop the trace; instead record the `SLOT_PROTECTIVE_READ` flag
    ///   and the read-only `storage_initial_values` cache, the inputs a
    ///   re-execution verifier derives the deduplicated set from.
    skip_storage_logs: bool,
}

#[derive(Debug)]
pub(crate) struct ExternalSnapshot {
    internal_snapshot: Snapshot,
    pub(crate) decommitted_hashes: <RollbackableMap<U256, DecommitState> as Rollback>::Snapshot,
    decommit_pinned_pages: <RollbackableSet<u32> as Rollback>::Snapshot,
    slot_flags: <RollbackableMap<(H160, U256), u8> as Rollback>::Snapshot,
    storage_refunds: <RollbackableLog<u32> as Rollback>::Snapshot,
    pubdata_costs: <RollbackableLog<i32> as Rollback>::Snapshot,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecommitState {
    /// A far-call decommit attempt ran out of gas before materialization.
    ///
    /// We preserve this state for legacy compatibility: old VM exposes these hashes as used
    /// contracts. This state is observable via `decommitted_hashes()`, but it must not make
    /// future decommits free.
    ///
    /// Note that `log.decommit` out-of-gas is not represented by this state because that opcode
    /// exits before decommit bookkeeping.
    #[default]
    Unsuccessful,
    /// A bytecode hash was successfully decommitted and has an assigned reusable heap page.
    Succeeded(u32),
}

impl WorldDiff {
    /// Controls whether the per-access storage log trace
    /// (`storage_logs` / `rollback_storage_logs`) is accumulated.
    ///
    /// Recording is **on by default** — it is required by consumers that build
    /// an in-circuit storage argument from `storage_log_queries()` (e.g. Boojum
    /// witness generation via `sort_storage_access_queries`). A re-execution
    /// verifier with no in-circuit storage argument (e.g. Airbender), which
    /// derives the deduplicated storage set from the `SLOT_PROTECTIVE_READ`
    /// flag + `storage_writes` instead, can pass `false` to avoid the trace's
    /// memory cost (~270 MiB on large batches).
    ///
    /// # Panics
    /// Panics if any storage slot has already been read or written — toggling
    /// mid-execution would leave a partial trace or partial dedup state.
    pub fn set_record_storage_logs(&mut self, record: bool) {
        assert!(
            self.storage_logs.is_empty()
                && self.storage_writes.as_ref().is_empty()
                && self.slot_flags.as_ref().is_empty()
                && self.storage_initial_values.is_empty(),
            "set_record_storage_logs must be called before any storage access"
        );
        self.skip_storage_logs = !record;
    }

    /// Reserve capacity for the auxiliary log Vecs (events, `pubdata_costs`,
    /// `storage_refunds`). Each of these doubles during execution and the
    /// transient peak is non-trivial inside the verifier guest.
    pub fn reserve_auxiliary_log_capacity(
        &mut self,
        events: usize,
        pubdata_costs: usize,
        storage_refunds: usize,
    ) {
        self.events.reserve(events);
        self.pubdata_costs.reserve(pubdata_costs);
        self.storage_refunds.reserve(storage_refunds);
    }

    /// Set `flag` on a slot's access-flags entry, returning `true` iff the flag
    /// was newly set (mirrors the former per-set `RollbackableSet::add`). The
    /// 52-byte `(address, key)` is stored once across all three flags. Single
    /// map traversal — see [`RollbackableMap::add_flags`].
    fn slot_add_flag(&mut self, key: (H160, U256), flag: u8) -> bool {
        self.slot_flags.add_flags(key, flag)
    }

    /// Returns the storage slot's value and a refund based on its hot/cold status.
    pub(crate) fn read_storage(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
        tx_number_in_block: u16,
    ) -> (U256, u32) {
        let (value, newly_added) =
            self.read_storage_inner(world, tracer, contract, key, tx_number_in_block);
        let refund = if !newly_added || world.is_free_storage_slot(&contract, &key) {
            WARM_READ_REFUND
        } else {
            0
        };
        self.storage_refunds.push(refund);
        (value, refund)
    }

    /// Same as [`Self::read_storage()`], but without recording the refund value (which is important
    /// because the storage is read not only from the `sload` op handler, but also from the `farcall` op handler;
    /// the latter must not record a refund as per previous VM versions).
    pub(crate) fn read_storage_without_refund(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
        tx_number_in_block: u16,
    ) -> U256 {
        self.read_storage_inner(world, tracer, contract, key, tx_number_in_block)
            .0
    }

    fn read_storage_inner(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
        tx_number_in_block: u16,
    ) -> (U256, bool) {
        let newly_added = self.slot_add_flag((contract, key), SLOT_READ);
        if newly_added {
            tracer.on_extra_prover_cycles(CycleStats::StorageRead);
        }

        self.pubdata_costs.push(0);
        let value = if self.skip_storage_logs {
            // Opt-out mode: no trace is kept, so record what the deduplicated set
            // is derived from instead — cache the initial value on first read
            // (writes already cache it) and flag a depth-zero read.
            let initial_value = self
                .storage_initial_values
                .entry((contract, key))
                .or_insert_with(|| world.read_storage(contract, key))
                .value;
            let live_write = self
                .storage_writes
                .as_ref()
                .get(&(contract, key))
                .map(|e| e.value);
            match live_write {
                Some(value) => value,
                None => {
                    // No pending write at read time: `did_read_at_depth_zero`.
                    self.slot_add_flag((contract, key), SLOT_PROTECTIVE_READ);
                    initial_value
                }
            }
        } else {
            // Recording mode: read like the pre-optimization base, without
            // caching read-only slots (keeps Boojum's memory unchanged).
            let value = self.just_read_storage(world, contract, key);
            // Record the per-access trace; read_value == written_value for a read.
            // Note: timestamp logic does not match `zk_evm`; we only ensure
            // timestamps are unique, which is fine as the witness is not
            // generated from these logs.
            self.storage_logs.push(LogQuery {
                timestamp: Timestamp(
                    u32::try_from(self.storage_logs.len()).expect("Too many storage logs"),
                ),
                tx_number_in_block,
                aux_byte: STORAGE_AUX_BYTE,
                shard_id: 0,
                address: contract,
                key,
                read_value: value,
                written_value: value,
                rw_flag: false,
                rollback: false,
                is_service: false,
            });
            value
        };
        (value, newly_added)
    }

    /// Reads the value of a storage slot without any extra bookkeeping.
    /// Should only be used for tracers.
    pub(crate) fn just_read_storage(
        &self,
        world: &mut impl StorageInterface,
        contract: H160,
        key: U256,
    ) -> U256 {
        self.storage_writes
            .as_ref()
            .get(&(contract, key))
            .map_or_else(|| world.read_storage_value(contract, key), |e| e.value)
    }

    /// Returns the refund based the hot/cold status of the storage slot and the change in pubdata.
    pub(crate) fn write_storage(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
        value: U256,
        tx_number_in_block: u16,
    ) -> u32 {
        if !self.skip_storage_logs {
            // Boojum mode: record the write and its rollback twin before the
            // change lands, so `read_value` is the pre-write value (matching
            // the legacy trace shape).
            let read_value = self.just_read_storage(world, contract, key);
            let log_query = LogQuery {
                timestamp: Timestamp(u32::try_from(self.storage_logs.len()).unwrap_or(u32::MAX)),
                tx_number_in_block,
                aux_byte: STORAGE_AUX_BYTE,
                shard_id: 0,
                address: contract,
                key,
                read_value,
                written_value: value,
                rw_flag: true,
                rollback: false,
                is_service: false,
            };
            self.storage_logs.push(log_query);
            self.rollback_storage_logs.push(LogQuery {
                rollback: true,
                ..log_query
            });
        }
        let initial_value = self
            .storage_initial_values
            .entry((contract, key))
            .or_insert_with(|| world.read_storage(contract, key));

        if world.is_free_storage_slot(&contract, &key) {
            // Free write: the value changes but no pubdata is paid, so the entry
            // keeps its prior paid amount. One journaling traversal — no
            // separate read-back of the prior entry.
            self.storage_writes
                .update((contract, key), |prev| StorageWriteEntry {
                    value,
                    paid: prev.map_or(0, |e| e.paid),
                });
            if self.slot_add_flag((contract, key), SLOT_WRITTEN) {
                tracer.on_extra_prover_cycles(CycleStats::StorageWrite);
            }
            self.slot_add_flag((contract, key), SLOT_READ);

            self.storage_refunds.push(WARM_WRITE_REFUND);
            self.pubdata_costs.push(0);
            return WARM_WRITE_REFUND;
        }

        let update_cost = world.cost_of_writing_storage(*initial_value, value);
        // Single insert with the final paid amount; `prepaid` (the prior paid)
        // comes from the replaced entry, avoiding a separate lookup.
        let prepaid = self
            .storage_writes
            .insert(
                (contract, key),
                StorageWriteEntry {
                    value,
                    paid: update_cost,
                },
            )
            .map_or(0, |e| e.paid);

        let refund = if self.slot_add_flag((contract, key), SLOT_WRITTEN) {
            tracer.on_extra_prover_cycles(CycleStats::StorageWrite);

            if self.slot_add_flag((contract, key), SLOT_READ) {
                0
            } else {
                COLD_WRITE_AFTER_WARM_READ_REFUND
            }
        } else {
            WARM_WRITE_REFUND
        };

        #[allow(clippy::cast_possible_wrap)]
        {
            let pubdata_cost = (update_cost as i32) - (prepaid as i32);
            self.pubdata.0 += pubdata_cost;
            self.storage_refunds.push(refund);
            self.pubdata_costs.push(pubdata_cost);
        }
        refund
    }

    pub(crate) fn pubdata(&self) -> i32 {
        self.pubdata.0
    }

    /// Returns recorded refunds for all storage operations.
    pub fn storage_refunds(&self) -> &[u32] {
        self.storage_refunds.as_ref()
    }

    /// Returns recorded pubdata costs for all storage operations.
    pub fn pubdata_costs(&self) -> &[i32] {
        self.pubdata_costs.as_ref()
    }

    /// Iterates over slots that need a *protective read* — read at rollback-depth
    /// zero (the dedup's `did_read_at_depth_zero` set). Combined with
    /// `storage_writes` this is the set of slots that appear in the deduplicated
    /// storage logs. Sorted by (address, key) via the `slot_flags` map's
    /// `BTreeMap` backing.
    pub fn protective_reads_iter(&self) -> impl Iterator<Item = (H160, U256)> + '_ {
        self.slot_flags
            .as_ref()
            .iter()
            .filter(|(_, f)| **f & SLOT_PROTECTIVE_READ != 0)
            .map(|(k, _)| *k)
    }

    /// Returns the initial (pre-batch) value of a slot if it has been
    /// touched by a read or write during execution. Used by per-slot summary
    /// derivation in place of walking the `storage_logs` trace.
    pub fn initial_storage_value(&self, contract: H160, key: U256) -> Option<crate::StorageSlot> {
        self.storage_initial_values.get(&(contract, key)).copied()
    }

    /// Returns all recorded storage log queries.
    ///
    /// These logs are sufficient for vm2 state-transition checks and diagnostics.
    // TODO: We don't fill all the `zk_evm` witness metadata, so this is not suitable for
    // generating EraVM prover witness data. This is not the goal, however, as we only need
    // to emit enough data to verify the correctness of the state transition.
    pub fn storage_log_queries(&self) -> &[LogQuery] {
        &self.storage_logs
    }

    /// Returns storage log queries recorded after the specified `snapshot` was created.
    pub fn storage_log_queries_after(&self, snapshot: &Snapshot) -> &[LogQuery] {
        &self.storage_logs[snapshot.storage_logs_len..]
    }

    #[doc(hidden)] // like `StateInterface::get_storage_state()` but exposes the full `StorageWriteEntry` (value + paid) for random access
    pub fn get_storage_state(&self) -> &BTreeMap<(H160, U256), StorageWriteEntry> {
        self.storage_writes.as_ref()
    }

    /// Gets changes for all touched storage slots.
    pub fn get_storage_changes(&self) -> impl Iterator<Item = ((H160, U256), StorageChange)> + '_ {
        self.storage_writes
            .as_ref()
            .iter()
            .filter_map(|(key, entry)| {
                let initial_slot = &self.storage_initial_values[key];
                if initial_slot.value == entry.value {
                    None
                } else {
                    Some((
                        *key,
                        StorageChange {
                            before: initial_slot.value,
                            after: entry.value,
                            is_initial: initial_slot.is_write_initial,
                        },
                    ))
                }
            })
    }

    /// Gets changes for storage slots touched after the specified `snapshot` was created.
    pub fn get_storage_changes_after(
        &self,
        snapshot: &Snapshot,
    ) -> impl Iterator<Item = ((H160, U256), StorageChange)> + '_ {
        self.storage_writes
            .changes_after(snapshot.storage_writes)
            .into_iter()
            .map(|(key, (before, after))| {
                let initial = self.storage_initial_values[&key];
                (
                    key,
                    StorageChange {
                        before: before.map_or(initial.value, |e| e.value),
                        after: after.value,
                        is_initial: initial.is_write_initial,
                    },
                )
            })
    }

    pub(crate) fn read_transient_storage(&mut self, contract: H160, key: U256) -> U256 {
        self.pubdata_costs.push(0);
        self.transient_storage_changes
            .as_ref()
            .get(&(contract, key))
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn write_transient_storage(&mut self, contract: H160, key: U256, value: U256) {
        self.pubdata_costs.push(0);
        self.transient_storage_changes
            .insert((contract, key), value);
    }

    pub(crate) fn get_transient_storage_state(&self) -> &BTreeMap<(H160, U256), U256> {
        self.transient_storage_changes.as_ref()
    }

    pub(crate) fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub(crate) fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    /// Returns events emitted after the specified `snapshot` was created.
    pub fn events_after(&self, snapshot: &Snapshot) -> &[Event] {
        self.events.logs_after(snapshot.events)
    }

    pub(crate) fn record_l2_to_l1_log(&mut self, log: L2ToL1Log) {
        self.l2_to_l1_logs.push(log);
    }

    pub(crate) fn l2_to_l1_logs(&self) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.as_ref()
    }

    /// Returns L2-to-L1 logs emitted after the specified `snapshot` was created.
    pub fn l2_to_l1_logs_after(&self, snapshot: &Snapshot) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.logs_after(snapshot.l2_to_l1_logs)
    }

    /// Returns hashes of contract bytecodes that were observed by decommit bookkeeping in no
    /// particular order.
    ///
    /// This includes successful decommits and far-call out-of-gas attempts recorded as
    /// [`DecommitState::Unsuccessful`] for legacy `used_contract_hashes` compatibility.
    pub fn decommitted_hashes(&self) -> impl Iterator<Item = U256> + '_ {
        self.decommitted_hashes.as_ref().keys().copied()
    }

    pub(crate) fn decommit_page(&self, code_hash: U256) -> Option<HeapId> {
        self.decommitted_hashes
            .as_ref()
            .get(&code_hash)
            .and_then(|state| {
                if let DecommitState::Succeeded(page) = state {
                    Some(HeapId::from_u32_unchecked(*page))
                } else {
                    None
                }
            })
    }

    pub(crate) fn is_decommit_page_pinned(&self, page: HeapId) -> bool {
        self.decommit_pinned_pages.as_ref().contains(&page.as_u32())
    }

    pub(crate) fn set_decommit_page(&mut self, code_hash: U256, page: HeapId) {
        self.decommitted_hashes
            .insert(code_hash, DecommitState::Succeeded(page.as_u32()));
        self.decommit_pinned_pages.add(page.as_u32());
    }

    /// Get a snapshot for selecting which logs & co. to output using [`Self::events_after()`] and other methods.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            storage_writes: self.storage_writes.snapshot(),
            events: self.events.snapshot(),
            l2_to_l1_logs: self.l2_to_l1_logs.snapshot(),
            transient_storage_changes: self.transient_storage_changes.snapshot(),
            pubdata: self.pubdata.snapshot(),
            storage_logs_len: self.storage_logs.len(),
            rollback_storage_logs_len: self.rollback_storage_logs.len(),
        }
    }

    /// Appends rollback storage logs recorded after `snapshot` to `storage_logs`.
    ///
    /// This is needed for failed frame returns (revert / panic) where rolled-back writes
    /// must remain observable in the storage log stream.
    pub(crate) fn append_rollback_logs(&mut self, snapshot: &Snapshot) {
        if self.rollback_storage_logs.len() > snapshot.rollback_storage_logs_len {
            let rollback_logs = self
                .rollback_storage_logs
                .split_off(snapshot.rollback_storage_logs_len);
            for log in rollback_logs.into_iter().rev() {
                self.storage_logs.push(log);
            }
        }
    }

    #[allow(clippy::needless_pass_by_value)] // intentional: we require a snapshot to be rolled back to no more than once
    pub(crate) fn rollback(&mut self, snapshot: Snapshot) {
        self.storage_writes.rollback(snapshot.storage_writes);
        self.events.rollback(snapshot.events);
        self.l2_to_l1_logs.rollback(snapshot.l2_to_l1_logs);
        self.transient_storage_changes
            .rollback(snapshot.transient_storage_changes);
        self.pubdata.rollback(snapshot.pubdata);
    }

    /// This function must only be called during the initial frame
    /// because otherwise internal rollbacks can roll back past the external snapshot.
    pub(crate) fn external_snapshot(&self) -> ExternalSnapshot {
        // Rolling back to this snapshot will clear transient storage even though it is not empty
        // after a transaction. This is ok because the next instruction in the bootloader
        // (IncrementTxNumber) clears the transient storage anyway.
        // This is necessary because clear_transient_storage cannot be undone.
        ExternalSnapshot {
            internal_snapshot: Snapshot {
                transient_storage_changes: 0,
                ..self.snapshot()
            },
            decommitted_hashes: self.decommitted_hashes.snapshot(),
            decommit_pinned_pages: self.decommit_pinned_pages.snapshot(),
            slot_flags: self.slot_flags.snapshot(),
            storage_refunds: self.storage_refunds.snapshot(),
            pubdata_costs: self.pubdata_costs.snapshot(),
        }
    }

    pub(crate) fn external_rollback(&mut self, snapshot: ExternalSnapshot) {
        let storage_logs_len = snapshot.internal_snapshot.storage_logs_len;
        let rollback_storage_logs_len = snapshot.internal_snapshot.rollback_storage_logs_len;

        self.rollback(snapshot.internal_snapshot);
        self.storage_refunds.rollback(snapshot.storage_refunds);
        self.pubdata_costs.rollback(snapshot.pubdata_costs);
        self.decommitted_hashes
            .rollback(snapshot.decommitted_hashes);
        self.decommit_pinned_pages
            .rollback(snapshot.decommit_pinned_pages);
        self.slot_flags.rollback(snapshot.slot_flags);
        self.storage_logs.truncate(storage_logs_len);
        self.rollback_storage_logs
            .truncate(rollback_storage_logs_len);
    }

    pub(crate) fn delete_history(&mut self) {
        self.storage_writes.delete_history();
        self.transient_storage_changes.delete_history();
        self.events.delete_history();
        self.l2_to_l1_logs.delete_history();
        self.pubdata.delete_history();
        self.storage_refunds.delete_history();
        self.pubdata_costs.delete_history();
        self.decommitted_hashes.delete_history();
        self.decommit_pinned_pages.delete_history();
        self.slot_flags.delete_history();
    }

    pub(crate) fn clear_transient_storage(&mut self) {
        self.transient_storage_changes = RollbackableMap::default();
    }
}

/// Opaque snapshot of a [`WorldDiff`] output by its [eponymous method](WorldDiff::snapshot()).
/// Can be provided to [`WorldDiff::events_after()`] etc. to get data after the snapshot was created.
#[derive(Clone, PartialEq, Debug)]
pub struct Snapshot {
    storage_writes: <RollbackableMap<(H160, U256), StorageWriteEntry> as Rollback>::Snapshot,
    events: <RollbackableLog<Event> as Rollback>::Snapshot,
    l2_to_l1_logs: <RollbackableLog<L2ToL1Log> as Rollback>::Snapshot,
    transient_storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    pubdata: <RollbackablePod<i32> as Rollback>::Snapshot,
    storage_logs_len: usize,
    rollback_storage_logs_len: usize,
}

/// Change in a single storage slot.
#[derive(Debug, PartialEq)]
pub struct StorageChange {
    /// Value before the slot was written to.
    pub before: U256,
    /// Value written to the slot.
    pub after: U256,
    /// `true` if the slot is not set in the [`World`](crate::World).
    /// A write may be initial even if it isn't the first write to a slot!
    pub is_initial: bool,
}

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;

#[cfg(test)]
mod tests {
    use proptest::{bits, collection::btree_map, prelude::*};

    use super::*;
    use crate::StorageSlot;

    fn test_storage_changes(
        initial_values: &BTreeMap<(H160, U256), StorageSlot>,
        first_changes: BTreeMap<(H160, U256), U256>,
        second_changes: BTreeMap<(H160, U256), U256>,
    ) {
        let mut world_diff = WorldDiff {
            storage_initial_values: initial_values.clone(),
            ..WorldDiff::default()
        };

        let checkpoint1 = world_diff.snapshot();
        for (key, value) in &first_changes {
            world_diff.write_storage(&mut NoWorld, &mut (), key.0, key.1, *value, 0);
        }
        let actual_changes = world_diff
            .get_storage_changes_after(&checkpoint1)
            .collect::<BTreeMap<_, _>>();
        let expected_changes = first_changes
            .iter()
            .map(|(key, value)| {
                let before = initial_values
                    .get(key)
                    .map_or_else(U256::zero, |slot| slot.value);
                let is_initial = initial_values
                    .get(key)
                    .is_none_or(|slot| slot.is_write_initial);
                (
                    *key,
                    StorageChange {
                        before,
                        after: *value,
                        is_initial,
                    },
                )
            })
            .collect();
        assert_eq!(actual_changes, expected_changes);

        let checkpoint2 = world_diff.snapshot();
        for (key, value) in &second_changes {
            world_diff.write_storage(&mut NoWorld, &mut (), key.0, key.1, *value, 0);
        }
        let actual_changes = world_diff
            .get_storage_changes_after(&checkpoint2)
            .collect::<BTreeMap<_, _>>();
        let expected_changes = second_changes
            .iter()
            .map(|(key, value)| {
                let before = first_changes
                    .get(key)
                    .or(initial_values.get(key).map(|slot| &slot.value))
                    .copied()
                    .unwrap_or_default();
                let is_initial = initial_values
                    .get(key)
                    .is_none_or(|slot| slot.is_write_initial);
                (
                    *key,
                    StorageChange {
                        before,
                        after: *value,
                        is_initial,
                    },
                )
            })
            .collect();
        assert_eq!(actual_changes, expected_changes);

        let mut combined = first_changes
            .into_iter()
            .filter_map(|(key, value)| {
                let initial = initial_values
                    .get(&key)
                    .copied()
                    .unwrap_or(StorageSlot::EMPTY);
                (initial.value != value).then_some((
                    key,
                    StorageChange {
                        before: initial.value,
                        after: value,
                        is_initial: initial.is_write_initial,
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();
        for (key, value) in second_changes {
            let initial = initial_values
                .get(&key)
                .copied()
                .unwrap_or(StorageSlot::EMPTY);
            if initial.value == value {
                combined.remove(&key);
            } else {
                combined.insert(
                    key,
                    StorageChange {
                        before: initial.value,
                        after: value,
                        is_initial: initial.is_write_initial,
                    },
                );
            }
        }

        assert_eq!(combined, world_diff.get_storage_changes().collect());
    }

    proptest! {
        #[test]
        fn storage_changes_work_as_expected(
            initial_values in arbitrary_initial_storage(),
            first_changes in arbitrary_storage_changes(),
            second_changes in arbitrary_storage_changes(),
        ) {
            test_storage_changes(&initial_values, first_changes, second_changes);
        }

        #[test]
        fn storage_changes_work_with_constrained_changes(
            initial_values in constrained_initial_storage(),
            first_changes in constrained_storage_changes(),
            second_changes in constrained_storage_changes(),
        ) {
            test_storage_changes(&initial_values, first_changes, second_changes);
        }
    }

    /// Max items in generated initial storage / changes.
    const MAX_ITEMS: usize = 5;
    /// Bit mask for bytes in constrained `U256` / `H160` values.
    const BIT_MASK: u8 = 0b_1111;

    fn arbitrary_initial_storage() -> impl Strategy<Value = BTreeMap<(H160, U256), StorageSlot>> {
        btree_map(
            any::<([u8; 20], [u8; 32])>()
                .prop_map(|(contract, key)| (H160::from(contract), U256::from(key))),
            any::<([u8; 32], bool)>().prop_map(|(value, is_write_initial)| StorageSlot {
                value: U256::from(value),
                is_write_initial,
            }),
            0..=MAX_ITEMS,
        )
    }

    fn constrained_initial_storage() -> impl Strategy<Value = BTreeMap<(H160, U256), StorageSlot>> {
        btree_map(
            (bits::u8::masked(BIT_MASK), bits::u8::masked(BIT_MASK))
                .prop_map(|(contract, key)| (H160::repeat_byte(contract), U256::from(key))),
            (bits::u8::masked(BIT_MASK), any::<bool>()).prop_map(|(value, is_write_initial)| {
                StorageSlot {
                    value: U256::from(value),
                    is_write_initial,
                }
            }),
            0..=MAX_ITEMS,
        )
    }

    fn arbitrary_storage_changes() -> impl Strategy<Value = BTreeMap<(H160, U256), U256>> {
        btree_map(
            any::<([u8; 20], [u8; 32])>()
                .prop_map(|(contract, key)| (H160::from(contract), U256::from(key))),
            any::<[u8; 32]>().prop_map(U256::from),
            0..=MAX_ITEMS,
        )
    }

    fn constrained_storage_changes() -> impl Strategy<Value = BTreeMap<(H160, U256), U256>> {
        btree_map(
            (bits::u8::masked(BIT_MASK), bits::u8::masked(BIT_MASK))
                .prop_map(|(contract, key)| (H160::repeat_byte(contract), U256::from(key))),
            bits::u8::masked(BIT_MASK).prop_map(U256::from),
            0..=MAX_ITEMS,
        )
    }

    struct NoWorld;

    impl StorageInterface for NoWorld {
        fn read_storage(&mut self, _: H160, _: U256) -> StorageSlot {
            StorageSlot::EMPTY
        }

        fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
            0
        }

        fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
            false
        }
    }

    /// A world with a fixed non-zero write cost, to exercise the merged `paid`
    /// accounting (`prior_paid` / `prepaid`) that `NoWorld`/`TestWorld` (cost 0)
    /// leave untested.
    struct CostWorld;

    impl StorageInterface for CostWorld {
        fn read_storage(&mut self, _: H160, _: U256) -> StorageSlot {
            StorageSlot::EMPTY
        }

        fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
            100
        }

        fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
            false
        }
    }

    #[test]
    fn merged_storage_write_tracks_paid_and_rolls_back() {
        let mut world_diff = WorldDiff::default();
        let (contract, key) = (H160::zero(), U256::from(1));

        // First write: prior_paid = 0, entry's paid set to the write cost.
        world_diff.write_storage(&mut CostWorld, &mut (), contract, key, U256::from(5), 0);
        let e = world_diff.storage_writes.as_ref()[&(contract, key)];
        assert_eq!((e.value, e.paid), (U256::from(5), 100));

        // Second write reuses prior_paid (=100) as `prepaid`; entry re-set.
        let snapshot = world_diff.snapshot();
        world_diff.write_storage(&mut CostWorld, &mut (), contract, key, U256::from(6), 0);
        let e = world_diff.storage_writes.as_ref()[&(contract, key)];
        assert_eq!((e.value, e.paid), (U256::from(6), 100));

        // Rollback restores both the value and the paid amount of the merged entry.
        world_diff.rollback(snapshot);
        let e = world_diff.storage_writes.as_ref()[&(contract, key)];
        assert_eq!((e.value, e.paid), (U256::from(5), 100));
    }

    #[derive(Default)]
    struct TestWorld {
        values: BTreeMap<(H160, U256), U256>,
    }

    impl StorageInterface for TestWorld {
        fn read_storage(&mut self, contract: H160, key: U256) -> StorageSlot {
            let value = self
                .values
                .get(&(contract, key))
                .copied()
                .unwrap_or_default();
            StorageSlot {
                value,
                is_write_initial: !self.values.contains_key(&(contract, key)),
            }
        }

        fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
            0
        }

        fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
            false
        }
    }

    #[test]
    fn storage_logs_include_reads_writes_and_rollbacks() {
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        let contract = H160::zero();
        let key = U256::from(1);

        let (value, _) = world_diff.read_storage(&mut world, &mut (), contract, key, 0);
        assert_eq!(value, U256::zero());

        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(10), 0);
        let snapshot = world_diff.snapshot();
        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(20), 0);
        world_diff.append_rollback_logs(&snapshot);
        world_diff.rollback(snapshot);

        let logs = world_diff.storage_log_queries();
        assert_eq!(logs.len(), 4);
        assert!(!logs[0].rw_flag);
        assert!(logs[1].rw_flag && !logs[1].rollback);
        assert!(logs[2].rw_flag && !logs[2].rollback);
        assert!(logs[3].rw_flag && logs[3].rollback);
        assert_eq!(logs[3].read_value, logs[2].read_value);
        assert_eq!(logs[3].written_value, logs[2].written_value);
    }

    #[test]
    fn skip_storage_logs_drops_trace_but_keeps_dedup_inputs() {
        let mut world_diff = WorldDiff::default();
        world_diff.set_record_storage_logs(false);
        let mut world = TestWorld::default();
        let contract = H160::zero();
        let key = U256::from(1);

        // Same access pattern as `storage_logs_include_reads_writes_and_rollbacks`.
        let (value, _) = world_diff.read_storage(&mut world, &mut (), contract, key, 0);
        assert_eq!(value, U256::zero());
        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(10), 0);
        let snapshot = world_diff.snapshot();
        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(20), 0);
        world_diff.append_rollback_logs(&snapshot);
        world_diff.rollback(snapshot);

        // The per-access trace is empty — the whole point of opting out.
        assert!(world_diff.storage_log_queries().is_empty());
        assert!(world_diff.rollback_storage_logs.is_empty());

        // ...but the maps a re-execution verifier derives the deduplicated set
        // from are still populated: the slot needs a protective read (read at
        // depth zero) and its initial value cached.
        assert!(world_diff
            .protective_reads_iter()
            .any(|slot| slot == (contract, key)));
        assert_eq!(
            world_diff
                .initial_storage_value(contract, key)
                .map(|s| s.value),
            Some(U256::zero())
        );
    }

    #[test]
    fn recording_mode_does_not_cache_read_only_slots() {
        // P2 regression: in default (recording / Boojum) mode a read-only slot
        // must not be added to `storage_initial_values`, so memory behavior
        // matches the pre-optimization base. The trace still records the read.
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        let (contract, key) = (H160::zero(), U256::from(1));

        let (value, _) = world_diff.read_storage(&mut world, &mut (), contract, key, 0);
        assert_eq!(value, U256::zero());

        assert_eq!(world_diff.storage_log_queries().len(), 1);
        assert!(world_diff.initial_storage_value(contract, key).is_none());
        assert!(world_diff
            .protective_reads_iter()
            .next()
            .is_none());
    }

    #[test]
    #[should_panic(expected = "before any storage access")]
    fn set_record_storage_logs_after_access_panics() {
        // P3 regression: the mode switch must reject being toggled once any
        // storage access has happened, rather than silently corrupting state.
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        world_diff.read_storage(&mut world, &mut (), H160::zero(), U256::from(1), 0);
        world_diff.set_record_storage_logs(false);
    }

    #[test]
    fn rollback_without_append_keeps_storage_log_stream_unchanged() {
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        let contract = H160::zero();
        let key = U256::from(1);

        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(10), 0);
        let snapshot = world_diff.snapshot();
        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(20), 0);
        world_diff.rollback(snapshot);

        let logs = world_diff.storage_log_queries();
        assert_eq!(logs.len(), 2);
        assert!(logs.iter().all(|log| log.rw_flag && !log.rollback));
        assert_eq!(world_diff.rollback_storage_logs.len(), 2);
    }

    #[test]
    fn external_rollback_truncates_storage_logs_to_internal_snapshot() {
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        let contract = H160::zero();
        let key = U256::from(1);

        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(10), 0);
        let snapshot = world_diff.external_snapshot();
        world_diff.write_storage(&mut world, &mut (), contract, key, U256::from(20), 0);

        world_diff.external_rollback(snapshot);

        let logs = world_diff.storage_log_queries();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].rw_flag && !logs[0].rollback);
        assert_eq!(world_diff.rollback_storage_logs.len(), 1);
    }

    #[test]
    fn storage_read_log_sets_written_value_to_read_value() {
        let mut world_diff = WorldDiff::default();
        let mut world = TestWorld::default();
        let contract = H160::repeat_byte(1);
        let key = U256::from(7);
        let value = U256::from(33);
        world.values.insert((contract, key), value);

        let (read_value, _) = world_diff.read_storage(&mut world, &mut (), contract, key, 0);
        assert_eq!(read_value, value);

        let logs = world_diff.storage_log_queries();
        assert_eq!(logs.len(), 1);
        assert!(!logs[0].rw_flag);
        assert_eq!(logs[0].read_value, value);
        assert_eq!(logs[0].written_value, value);
    }
}
