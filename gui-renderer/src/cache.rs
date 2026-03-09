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
    /// Create a cache that holds at most `max_entries` documents.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            max_entries,
        }
    }

    /// Look up a cached UIDL document by key, updating its access time.
    pub fn get(&self, key: &str) -> Option<UidlDocument> {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(entry) = entries.get_mut(key) {
            entry.last_accessed = Instant::now();
            Some(entry.document.clone())
        } else {
            None
        }
    }

    /// Insert a document, evicting the least-recently-accessed entry if at capacity.
    pub fn insert(&self, key: CacheKey, document: UidlDocument) {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
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

    /// Remove a single entry by key.
    pub fn invalidate(&self, key: &str) {
        match self.entries.lock() {
            Ok(mut guard) => {
                guard.remove(key);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(key);
            }
        }
    }

    /// Remove all entries.
    pub fn clear(&self) {
        match self.entries.lock() {
            Ok(mut guard) => guard.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }

    /// Return the number of cached entries.
    pub fn len(&self) -> usize {
        match self.entries.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Returns `true` if the cache contains no entries.
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

    #[test]
    fn test_cache_invalidate() {
        let cache = UidlCache::new(10);
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "x"}}"#).unwrap();
        cache.insert("k1".to_string(), doc);
        assert!(cache.get("k1").is_some());
        cache.invalidate("k1");
        assert!(cache.get("k1").is_none());
    }

    #[test]
    fn test_cache_clear() {
        let cache = UidlCache::new(10);
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "x"}}"#).unwrap();
        cache.insert("k1".to_string(), doc.clone());
        cache.insert("k2".to_string(), doc);
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_overwrite_same_key() {
        let cache = UidlCache::new(10);
        let doc1 =
            UidlDocument::parse(r#"{"layout": {"type": "text", "content": "first"}}"#).unwrap();
        let doc2 =
            UidlDocument::parse(r#"{"layout": {"type": "text", "content": "second"}}"#).unwrap();
        cache.insert("k1".to_string(), doc1);
        cache.insert("k1".to_string(), doc2.clone());
        let retrieved = cache.get("k1").unwrap();
        assert_eq!(retrieved, doc2);
    }

    #[test]
    fn test_cache_lru_evicts_oldest() {
        let cache = UidlCache::new(2);
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "x"}}"#).unwrap();
        cache.insert("oldest".to_string(), doc.clone());
        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
        cache.insert("newer".to_string(), doc.clone());
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Access "oldest" to make it recently used
        cache.get("oldest");
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Insert third - should evict "newer" since "oldest" was accessed more recently
        cache.insert("newest".to_string(), doc);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("oldest").is_some());
        assert!(cache.get("newest").is_some());
        assert!(cache.get("newer").is_none());
    }

    #[test]
    fn test_cache_capacity_one() {
        let cache = UidlCache::new(1);
        let doc = UidlDocument::parse(r#"{"layout": {"type": "text", "content": "x"}}"#).unwrap();
        cache.insert("k1".to_string(), doc.clone());
        cache.insert("k2".to_string(), doc);
        assert_eq!(cache.len(), 1);
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k1").is_none());
    }
}
