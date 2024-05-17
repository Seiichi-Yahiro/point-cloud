use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::hash::Hash;

#[derive(Copy, Clone)]
pub struct SortedHashKey<K, SK>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    pub hash_key: K,
    pub sort_key: SK,
}

impl<K, SK> PartialEq for SortedHashKey<K, SK>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    fn eq(&self, other: &Self) -> bool {
        self.sort_key.eq(&other.sort_key)
    }
}

impl<K, SK> Eq for SortedHashKey<K, SK>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
}

impl<K, SK> PartialOrd for SortedHashKey<K, SK>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K, SK> Ord for SortedHashKey<K, SK>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key.cmp(&other.sort_key)
    }
}

pub struct SortedHashEntry<K, SK, V>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    pub keys: SortedHashKey<K, SK>,
    pub value: V,
}

pub struct SortedHashMap<K, SK, V>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    map: FxHashMap<K, SortedHashEntry<K, SK, V>>,
    sorted_set: BTreeSet<SortedHashKey<K, SK>>,
}

impl<K, SK, V> SortedHashMap<K, SK, V>
where
    K: Hash + Eq + Copy,
    SK: Ord + Eq + Copy,
{
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            sorted_set: BTreeSet::default(),
        }
    }

    pub fn insert(&mut self, hash_key: K, sort_key: SK, value: V) {
        let keys = SortedHashKey { hash_key, sort_key };

        let entry = SortedHashEntry { keys, value };

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

    pub fn clear(&mut self) {
        self.map.clear();
        self.sorted_set.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}
