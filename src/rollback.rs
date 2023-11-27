use std::collections::BTreeMap;

/// A trait for things that can be rolled back to snapshots
pub(crate) trait Rollback {
    type Snapshot;
    fn snapshot(&self) -> Self::Snapshot;
    fn rollback(&mut self, snapshot: Self::Snapshot);
    fn forget(&mut self, snapshot: Self::Snapshot);
}

#[derive(Default)]
pub struct RollbackableMap<K: Ord, V> {
    map: BTreeMap<K, V>,
    old_entries: Vec<(K, Option<V>)>,
}

impl<K: Ord + Clone, V> RollbackableMap<K, V> {
    pub fn insert(&mut self, key: K, value: V, permanent_change: bool) {
        if permanent_change {
            self.map.insert(key, value);
        } else {
            self.old_entries
                .push((key.clone(), self.map.insert(key, value)));
        }
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

    fn forget(&mut self, snapshot: Self::Snapshot) {
        self.old_entries.truncate(snapshot)
    }
}

impl<K: Ord, V> AsRef<BTreeMap<K, V>> for RollbackableMap<K, V> {
    fn as_ref(&self) -> &BTreeMap<K, V> {
        &self.map
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

    fn forget(&mut self, _: Self::Snapshot) {}
}

impl<T> RollbackableLog<T> {
    pub fn push(&mut self, entry: T) {
        self.entries.push(entry)
    }
}

impl<T> AsRef<[T]> for RollbackableLog<T> {
    fn as_ref(&self) -> &[T] {
        &self.entries
    }
}
