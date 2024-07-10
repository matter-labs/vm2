use ahash::HashMap;
use std::hash::Hash;

/// A trait for things that can be rolled back to snapshots
pub(crate) trait Rollback {
    type Snapshot;
    fn snapshot(&self) -> Self::Snapshot;
    fn rollback(&mut self, snapshot: Self::Snapshot);
    fn delete_history(&mut self);
}

#[derive(Default)]
pub struct RollbackableMap<K, V> {
    map: HashMap<K, V>,
    old_entries: Vec<(K, Option<V>)>,
}

impl<K: Eq + Hash + Copy, V: Copy> RollbackableMap<K, V> {
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let old_value = self.map.insert(key, value);
        self.old_entries.push((key, old_value));
        old_value
    }

    pub(crate) fn changes_after(
        &self,
        snapshot: <Self as Rollback>::Snapshot,
    ) -> HashMap<K, (Option<V>, V)> {
        let mut changes = HashMap::default();
        for &(key, old_value) in self.old_entries[snapshot..].iter().rev() {
            changes
                .entry(key)
                .and_modify(|(old, _)| *old = old_value)
                .or_insert((old_value, self.map[&key]));
        }
        changes
    }
}

impl<K: Eq + Hash + Copy, V: Copy> Rollback for RollbackableMap<K, V> {
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

impl<K, V> AsRef<HashMap<K, V>> for RollbackableMap<K, V> {
    fn as_ref(&self) -> &HashMap<K, V> {
        &self.map
    }
}

pub type RollbackableSet<T> = RollbackableMap<T, ()>;

impl<T: Eq + Hash + Copy> RollbackableSet<T> {
    pub fn add(&mut self, key: T) {
        self.insert(key, ());
    }

    pub fn contains(&self, key: &T) -> bool {
        self.as_ref().contains_key(key)
    }
}

pub struct RollbackableLog<T> {
    entries: Vec<T>,
}

impl<T> Default for RollbackableLog<T> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
        }
    }
}

impl<T> Rollback for RollbackableLog<T> {
    type Snapshot = usize;

    fn snapshot(&self) -> Self::Snapshot {
        self.entries.len()
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        self.entries.truncate(snapshot)
    }

    fn delete_history(&mut self) {}
}

impl<T> RollbackableLog<T> {
    pub fn push(&mut self, entry: T) {
        self.entries.push(entry)
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
#[derive(Default, Copy, Clone)]
pub struct RollbackablePod<T: Copy>(pub T);

impl<T: Copy> Rollback for RollbackablePod<T> {
    type Snapshot = T;

    fn snapshot(&self) -> Self::Snapshot {
        self.0
    }

    fn rollback(&mut self, snapshot: Self::Snapshot) {
        self.0 = snapshot
    }

    fn delete_history(&mut self) {}
}
