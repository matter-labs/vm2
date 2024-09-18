use std::collections::{BTreeMap, BTreeSet};

/// A trait for things that can be rolled back to snapshots
pub(crate) trait Rollback {
    type Snapshot;
    fn snapshot(&self) -> Self::Snapshot;
    fn rollback(&mut self, snapshot: Self::Snapshot);
    fn delete_history(&mut self);
}

#[derive(Debug, Default)]
pub(crate) struct RollbackableMap<K: Ord, V> {
    map: BTreeMap<K, V>,
    old_entries: Vec<(K, Option<V>)>,
}

impl<K: Ord + Clone, V: Clone> RollbackableMap<K, V> {
    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let old_value = self.map.insert(key.clone(), value);
        self.old_entries.push((key, old_value.clone()));
        old_value
    }

    pub(crate) fn changes_after(
        &self,
        snapshot: <Self as Rollback>::Snapshot,
    ) -> BTreeMap<K, (Option<V>, V)> {
        let mut changes = BTreeMap::new();
        for (key, old_value) in self.old_entries[snapshot..].iter().rev() {
            changes
                .entry(key.clone())
                .and_modify(|(old, _): &mut (Option<V>, V)| old.clone_from(old_value))
                .or_insert((old_value.clone(), self.map.get(key).unwrap().clone()));
        }
        changes
    }
}

impl<K: Ord, V> Rollback for RollbackableMap<K, V> {
    type Snapshot = usize;

    fn snapshot(&self) -> Self::Snapshot {
        self.old_entries.len()
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        for (k, v) in self.old_entries.drain(snapshot..).rev() {
            if let Some(old_value) = v {
                self.map.insert(k, old_value);
            } else {
                self.map.remove(&k);
            }
        }
    }

    fn delete_history(&mut self) {
        self.old_entries.clear();
    }
}

impl<K: Ord, V> AsRef<BTreeMap<K, V>> for RollbackableMap<K, V> {
    fn as_ref(&self) -> &BTreeMap<K, V> {
        &self.map
    }
}

#[derive(Debug, Default)]
pub(crate) struct RollbackableSet<K: Ord> {
    map: BTreeSet<K>,
    old_entries: Vec<K>,
}

impl<T: Ord + Clone> RollbackableSet<T> {
    /// Adds `key` to the set and returns if it was added (not present earlier).
    pub(crate) fn add(&mut self, key: T) -> bool {
        let is_new = self.map.insert(key.clone());
        if is_new {
            self.old_entries.push(key);
        }
        is_new
    }
}

impl<K: Ord> Rollback for RollbackableSet<K> {
    type Snapshot = usize;

    fn snapshot(&self) -> Self::Snapshot {
        self.old_entries.len()
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        for k in self.old_entries.drain(snapshot..) {
            self.map.remove(&k);
        }
    }

    fn delete_history(&mut self) {
        self.old_entries.clear();
    }
}

impl<K: Ord> AsRef<BTreeSet<K>> for RollbackableSet<K> {
    fn as_ref(&self) -> &BTreeSet<K> {
        &self.map
    }
}

#[derive(Debug)]
pub(crate) struct RollbackableLog<T> {
    entries: Vec<T>,
}

impl<T> Default for RollbackableLog<T> {
    fn default() -> Self {
        Self {
            entries: Vec::default(),
        }
    }
}

impl<T> Rollback for RollbackableLog<T> {
    type Snapshot = usize;

    fn snapshot(&self) -> Self::Snapshot {
        self.entries.len()
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        self.entries.truncate(snapshot);
    }

    fn delete_history(&mut self) {}
}

impl<T> RollbackableLog<T> {
    pub(crate) fn push(&mut self, entry: T) {
        self.entries.push(entry);
    }

    pub(crate) fn logs_after(&self, snapshot: <RollbackableLog<T> as Rollback>::Snapshot) -> &[T] {
        &self.entries[snapshot..]
    }
}

impl<T> AsRef<[T]> for RollbackableLog<T> {
    fn as_ref(&self) -> &[T] {
        &self.entries
    }
}

/// Rollbackable Plain Old Data simply stores copies of itself in snapshots.
#[derive(Debug, Default, Copy, Clone)]
pub(crate) struct RollbackablePod<T: Copy>(pub(crate) T);

impl<T: Copy> Rollback for RollbackablePod<T> {
    type Snapshot = T;

    fn snapshot(&self) -> Self::Snapshot {
        self.0
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        self.0 = snapshot;
    }

    fn delete_history(&mut self) {}
}
