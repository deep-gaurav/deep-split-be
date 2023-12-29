use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::time::{Duration, Instant};

#[derive(Eq, PartialEq)]
struct ExpiringEntry<K> {
    key: K,
    expiration: Instant,
}

impl<K> Ord for ExpiringEntry<K>
where
    K: Eq,
{
    fn cmp(&self, other: &Self) -> Ordering {
        other.expiration.cmp(&self.expiration)
    }
}

impl<K> PartialOrd for ExpiringEntry<K>
where
    K: Eq,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct ExpiringHashMap<K, V> {
    values: HashMap<K, V>,
    expirations: BinaryHeap<ExpiringEntry<K>>,
    default_ttl: Duration,
}

impl<K, V> ExpiringHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    pub fn new(default_ttl: Duration) -> Self {
        ExpiringHashMap {
            values: HashMap::new(),
            expirations: BinaryHeap::new(),
            default_ttl,
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        let expiration = Instant::now() + self.default_ttl;
        if let Some(_old) = self.values.insert(key.clone(), value) {
            self.expirations.retain(|entry| entry.key != key);
        }
        self.expirations.push(ExpiringEntry { key, expiration });
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.cleanup_expired_keys();
        self.values.get(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.values.remove(key);
        self.expirations.retain(|entry| &entry.key != key);
        None
    }

    pub fn contains_key(&mut self, key: &K) -> bool {
        self.cleanup_expired_keys();
        self.values.contains_key(key)
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.expirations.clear();
    }

    fn cleanup_expired_keys(&mut self) {
        let now = Instant::now();
        while let Some(entry) = self.expirations.peek() {
            if entry.expiration <= now {
                let expired_entry = self.expirations.pop().unwrap();
                self.values.remove(&expired_entry.key);
            } else {
                break;
            }
        }
    }
}
