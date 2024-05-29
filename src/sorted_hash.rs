use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::{BuildHasher, BuildHasherDefault, Hash};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortedHashKey<K, SK> {
    pub sort_key: SK,
    pub hash_key: K,
}

impl<K, SK> PartialOrd for SortedHashKey<K, SK>
where
    K: Eq + Hash,
    SK: PartialOrd + Ord,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K, SK> Ord for SortedHashKey<K, SK>
where
    K: Eq + Hash,
    SK: Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        match self.sort_key.cmp(&other.sort_key) {
            Ordering::Equal => {
                let h = BuildHasherDefault::<FxHasher>::default();
                let h1 = h.hash_one(&self.hash_key);
                let h2 = h.hash_one(&other.hash_key);

                h1.cmp(&h2)
            }
            cmp => cmp,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct SortedHashEntry<K, SK, V> {
    pub keys: Arc<SortedHashKey<K, SK>>,
    pub value: V,
}

#[derive(Debug)]
pub struct SortedHashMap<K, SK, V> {
    map: FxHashMap<K, SortedHashEntry<K, SK, V>>,
    sorted_set: BTreeSet<Arc<SortedHashKey<K, SK>>>,
}

impl<K, SK, V> SortedHashMap<K, SK, V> {
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            sorted_set: BTreeSet::default(),
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.sorted_set.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

impl<K, SK, V> SortedHashMap<K, SK, V>
where
    K: Hash + Eq + Copy,
    SK: Ord,
{
    pub fn insert(&mut self, hash_key: K, sort_key: SK, value: V) {
        let keys = Arc::new(SortedHashKey { hash_key, sort_key });

        let entry = SortedHashEntry {
            keys: Arc::clone(&keys),
            value,
        };

        if let Some(old_entry) = self.map.insert(hash_key, entry) {
            self.sorted_set.remove(&old_entry.keys);
        }

        self.sorted_set.insert(keys);
    }

    pub fn remove(&mut self, hash_key: &K) -> Option<SortedHashEntry<K, SK, V>> {
        if let Some(entry) = self.map.remove(hash_key) {
            self.sorted_set.remove(&entry.keys);
            Some(entry)
        } else {
            None
        }
    }

    pub fn pop_first(&mut self) -> Option<SortedHashEntry<K, SK, V>> {
        if let Some(keys) = self.sorted_set.pop_first() {
            self.map.remove(&keys.hash_key)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_same_hash_key_twice_overrides() {
        let mut shm = SortedHashMap::<u32, u32, ()>::new();

        shm.insert(0, 0, ());
        shm.insert(0, 1, ());

        assert_eq!(
            shm.map.len(),
            shm.sorted_set.len(),
            "lengths don't match, hash: {}, sort: {}",
            shm.map.len(),
            shm.sorted_set.len()
        );

        assert_eq!(shm.len(), 1, "total length should be 1");
    }

    #[test]
    fn can_hold_same_sort_key_twice() {
        let mut shm = SortedHashMap::<u32, u32, ()>::new();

        shm.insert(0, 0, ());
        shm.insert(1, 0, ());

        assert_eq!(
            shm.map.len(),
            shm.sorted_set.len(),
            "lengths don't match, hash: {}, sort: {}",
            shm.map.len(),
            shm.sorted_set.len()
        );

        assert_eq!(shm.len(), 2, "total length should be 2");
    }

    #[test]
    fn returns_sorted() {
        let mut shm = SortedHashMap::<u32, u32, ()>::new();

        shm.insert(0, 2, ());
        shm.insert(1, 3, ());
        shm.insert(2, 1, ());

        let first = shm.pop_first();
        let second = shm.pop_first();
        let third = shm.pop_first();
        let last = shm.pop_first();

        let result = [first, second, third, last];
        let expected = [
            Some(SortedHashEntry {
                keys: Arc::new(SortedHashKey {
                    hash_key: 2,
                    sort_key: 1,
                }),
                value: (),
            }),
            Some(SortedHashEntry {
                keys: Arc::new(SortedHashKey {
                    hash_key: 0,
                    sort_key: 2,
                }),
                value: (),
            }),
            Some(SortedHashEntry {
                keys: Arc::new(SortedHashKey {
                    hash_key: 1,
                    sort_key: 3,
                }),
                value: (),
            }),
            None,
        ];

        assert_eq!(result, expected);

        assert_eq!(
            shm.map.len(),
            shm.sorted_set.len(),
            "lengths don't match, hash: {}, sort: {}",
            shm.map.len(),
            shm.sorted_set.len()
        );

        assert_eq!(shm.len(), 0, "total length should be 0");
    }

    #[test]
    fn remove_by_hash_key() {
        let mut shm = SortedHashMap::<u32, u32, ()>::new();

        shm.insert(0, 2, ());
        shm.insert(1, 3, ());
        shm.insert(2, 1, ());

        shm.remove(&1);

        let first = shm.pop_first();
        let second = shm.pop_first();
        let last = shm.pop_first();

        let result = [first, second, last];
        let expected = [
            Some(SortedHashEntry {
                keys: Arc::new(SortedHashKey {
                    hash_key: 2,
                    sort_key: 1,
                }),
                value: (),
            }),
            Some(SortedHashEntry {
                keys: Arc::new(SortedHashKey {
                    hash_key: 0,
                    sort_key: 2,
                }),
                value: (),
            }),
            None,
        ];

        assert_eq!(result, expected);

        assert_eq!(
            shm.map.len(),
            shm.sorted_set.len(),
            "lengths don't match, hash: {}, sort: {}",
            shm.map.len(),
            shm.sorted_set.len()
        );

        assert_eq!(shm.len(), 0, "total length should be 0");
    }
}
