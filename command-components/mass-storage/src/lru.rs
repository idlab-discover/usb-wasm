use ahash::AHashMap;
use std::{collections::HashMap, fmt::Debug, hash::Hash};

pub trait HashKey: Hash + PartialEq + Eq + Clone + Debug {}
impl<T: Hash + PartialEq + Eq + Clone + Debug> HashKey for T {}

struct LruEntry<V> {
    last_used: usize,
    value: V,
}

pub struct LruCache<K: HashKey, V: Debug> {
    capacity: usize,
    cache: AHashMap<K, LruEntry<V>>,
    age: usize,
}

impl<K: HashKey, V: Debug> LruCache<K, V> {
    fn remove_lru_entry(&mut self) -> Option<(K, V)> {
        if self.cache.len() == 0 {
            return None;
        }

        let key = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(k, _)| k)
            .unwrap()
            .clone();
        let entry = self.cache.remove_entry(&key).unwrap();

        // println!("Ejecting {:?}", key);

        Some((entry.0, entry.1.value))
    }

    pub fn new(capacity: usize) -> Self {
        LruCache {
            capacity,
            cache: AHashMap::with_capacity(capacity),
            age: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        // println!("Cache: get({:?})", key);
        self.age += 1;
        let mut entry = self.cache.get_mut(key);

        if let Some(entry) = &mut entry {
            entry.last_used = self.age;
        }

        entry.map(|e| &e.value)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        // println!("Cache: get_mut({:?})", key);
        self.age += 1;
        let mut entry = self.cache.get_mut(key);

        if let Some(entry) = &mut entry {
            entry.last_used = self.age;
        }

        entry.map(|e| &mut e.value)
    }

    // If a value had to be evicted, returns the evicted key and value
    pub fn set(&mut self, key: K, value: V) -> Option<(K, V)> {
        // println!("Cache: set({:?}) (contains: {})", key, self.cache.contains_key(&key));
        self.age += 1;

        if self.cache.contains_key(&key) {
            let entry = self.cache.get_mut(&key).unwrap();
            entry.last_used = self.age;
            entry.value = value;
            return None;
        }

        let evicted_entry = if self.len() >= self.capacity {
            // println!("Cache: evicting");
            self.remove_lru_entry()
        } else {
            None
        };

        self.cache.insert(
            key.clone(),
            LruEntry {
                value,
                last_used: self.age,
            },
        );

        evicted_entry
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        // println!("Cache: remove({:?})", key);
        self.cache.remove(key).map(|e| e.value)
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.cache.iter().map(|(k, v)| (k, &v.value))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.cache.iter_mut().map(|(k, v)| (k, &mut v.value))
    }
}
