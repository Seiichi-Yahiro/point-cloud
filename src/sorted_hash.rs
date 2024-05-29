use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::sync::Arc;

pub struct SortedHashKey<K, SK> {
    pub hash_key: K,
    pub sort_key: SK,
}

impl<K, SK> Debug for SortedHashKey<K, SK>
where
    K: Debug,
    SK: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SortedHashKey")
            .field("hash_key", &self.hash_key)
            .field("sort_key", &self.sort_key)
            .finish()
    }
}

impl<K, SK> PartialEq for SortedHashKey<K, SK>
where
    SK: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.sort_key.eq(&other.sort_key)
    }
}

impl<K, SK> Eq for SortedHashKey<K, SK> where SK: Eq {}

impl<K, SK> PartialOrd for SortedHashKey<K, SK>
where
    SK: PartialOrd + Ord,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K, SK> Ord for SortedHashKey<K, SK>
where
    SK: Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key.cmp(&other.sort_key)
    }
}

pub struct SortedHashEntry<K, SK, V> {
    pub keys: Arc<SortedHashKey<K, SK>>,
    pub value: V,
}

impl<K, SK, V> Debug for SortedHashEntry<K, SK, V>
where
    K: Debug,
    SK: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SortedHashEntry")
            .field("keys", &self.keys)
            .field("value", &self.value)
            .finish()
    }
}

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

impl<K, SK, V> Debug for SortedHashMap<K, SK, V>
where
    K: Debug,
    SK: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SortedHashMap")
            .field("map", &self.map)
            .field("sorted_set", &self.sorted_set)
            .finish()
    }
}
