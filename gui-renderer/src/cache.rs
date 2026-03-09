use crate::uidl::UidlDocument;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Cache key: SHA256(AppSpec + state)
pub type CacheKey = String;

struct CacheEntry {
    document: UidlDocument,
    last_accessed: Instant,
}

/// UIDL cache — avoids re-generating UI when state hasn't semantically changed
/// Evict least-recently-accessed entry when at capacity.
#[derive(Clone)]
pub struct UidlCache {
    entries: Arc<Mutex<HashMap<CacheKey, CacheEntry>>>,
    max_entries: usize,
}

impl UidlCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            max_entries,
        }
    }

    pub fn get(&self, key: &str) -> Option<UidlDocument> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(key) {
            entry.last_accessed = Instant::now();
            Some(entry.document.clone())
        } else {
            None
        }
    }

    pub fn insert(&self, key: CacheKey, document: UidlDocument) {
        let mut entries = self.entries.lock().unwrap();
        // Evict least-recently-accessed if at capacity
        if entries.len() >= self.max_entries {
            let lru_key = entries
                .iter()
                .min_by_key(|(_, v)| v.last_accessed)
                .map(|(k, _)| k.clone());
            if let Some(k) = lru_key {
                entries.remove(&k);
            }
        }
        entries.insert(
            key,
            CacheEntry {
                document,
                last_accessed: Instant::now(),
            },
        );
    }

    pub fn invalidate(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uidl::UidlDocument;

    #[test]
    fn test_cache_lru_eviction() {
        let cache = UidlCache::new(2);
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "x"}}"#).unwrap();
        cache.insert("key1".to_string(), doc.clone());
        cache.insert("key2".to_string(), doc.clone());
        cache.insert("key3".to_string(), doc.clone());
        assert_eq!(cache.len(), 2); // max_entries=2
    }

    #[test]
    fn test_cache_get_returns_none_for_missing() {
        let cache = UidlCache::new(10);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = UidlCache::new(10);
        let doc =
            UidlDocument::parse(r#"{"layout": {"type": "text", "content": "hello"}}"#).unwrap();
        cache.insert("k1".to_string(), doc.clone());
        let retrieved = cache.get("k1");
        assert!(retrieved.is_some());
    }
}
