//! Link Store - Cross-memory ontology edges
//!
//! Stores typed links between memory records across all memory types.
//! Links form the dynamic ontology that agents learn through experience.

use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use adb_core::{AdbResult, Link, LinkPredicate, MemoryRecord, MemoryType};

/// Trait for link storage implementations
#[async_trait]
pub trait LinkStoreOps: Send + Sync {
    /// Create a link between two records
    async fn link(
        &self,
        from_type: MemoryType,
        from_id: &str,
        to_type: MemoryType,
        to_id: &str,
        link_type: &str,
        weight: f32,
    ) -> AdbResult<Link>;

    /// Get links from a record
    async fn get_links_from(
        &self,
        from_type: MemoryType,
        from_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>>;

    /// Get links to a record
    async fn get_links_to(
        &self,
        to_type: MemoryType,
        to_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>>;

    /// Get all links matching a predicate
    async fn get_links(&self, predicate: &LinkPredicate) -> AdbResult<Vec<Link>>;

    /// Update link weight
    async fn update_weight(&self, link_id: &str, weight: f32) -> AdbResult<()>;

    /// Update weight with success/failure signal using EMA
    async fn update_weight_with_signal(
        &self,
        link_id: &str,
        signal: f32,
        decay: f32,
    ) -> AdbResult<()>;

    /// Delete links matching predicate
    async fn forget_links(&self, predicate: &LinkPredicate) -> AdbResult<u64>;

    /// Get link count
    async fn count(&self) -> usize;

    /// Clear all links
    async fn clear(&self) -> AdbResult<()>;
}

/// In-memory link store using DashMap
///
/// Uses multiple indexes for efficient querying:
/// - by_id: link_id -> Link
/// - by_from: (from_type, from_id) -> [link_id]
/// - by_to: (to_type, to_id) -> [link_id]
/// - by_type: link_type -> [link_id]
pub struct LinkStore {
    /// Primary storage by link ID
    by_id: DashMap<String, Link>,
    /// Index by source (from_type, from_id)
    by_from: DashMap<(MemoryType, String), Vec<String>>,
    /// Index by target (to_type, to_id)
    by_to: DashMap<(MemoryType, String), Vec<String>>,
    /// Index by link type
    by_type: DashMap<String, Vec<String>>,
    /// Link counter for stats
    link_count: AtomicU64,
}

impl LinkStore {
    /// Create a new link store
    pub fn new() -> Self {
        Self {
            by_id: DashMap::new(),
            by_from: DashMap::new(),
            by_to: DashMap::new(),
            by_type: DashMap::new(),
            link_count: AtomicU64::new(0),
        }
    }

    /// Add link to all indexes
    fn index_link(&self, link: &Link) {
        let id = link.id.clone();

        // Add to from index
        self.by_from
            .entry((link.from_type, link.from_id.clone()))
            .or_default()
            .push(id.clone());

        // Add to to index
        self.by_to
            .entry((link.to_type, link.to_id.clone()))
            .or_default()
            .push(id.clone());

        // Add to type index
        self.by_type
            .entry(link.link_type.clone())
            .or_default()
            .push(id);
    }

    /// Remove link from all indexes
    fn unindex_link(&self, link: &Link) {
        let id = &link.id;

        // Remove from 'from' index
        if let Some(mut entry) = self.by_from.get_mut(&(link.from_type, link.from_id.clone())) {
            entry.retain(|x| x != id);
        }

        // Remove from 'to' index
        if let Some(mut entry) = self.by_to.get_mut(&(link.to_type, link.to_id.clone())) {
            entry.retain(|x| x != id);
        }

        // Remove from type index
        if let Some(mut entry) = self.by_type.get_mut(&link.link_type) {
            entry.retain(|x| x != id);
        }
    }

    /// Get links by IDs
    fn get_links_by_ids(&self, ids: &[String]) -> Vec<Link> {
        ids.iter()
            .filter_map(|id| self.by_id.get(id).map(|r| r.value().clone()))
            .collect()
    }

    /// Get link type statistics
    pub fn type_stats(&self) -> HashMap<String, usize> {
        self.by_type
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().len()))
            .collect()
    }

    /// Get all unique link types
    pub fn link_types(&self) -> Vec<String> {
        self.by_type.iter().map(|e| e.key().clone()).collect()
    }
}

impl Default for LinkStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LinkStoreOps for LinkStore {
    async fn link(
        &self,
        from_type: MemoryType,
        from_id: &str,
        to_type: MemoryType,
        to_id: &str,
        link_type: &str,
        weight: f32,
    ) -> AdbResult<Link> {
        let link = Link::new(from_type, from_id, to_type, to_id, link_type, weight);

        // Add to primary storage
        self.by_id.insert(link.id.clone(), link.clone());

        // Add to indexes
        self.index_link(&link);

        self.link_count.fetch_add(1, Ordering::SeqCst);

        Ok(link)
    }

    async fn get_links_from(
        &self,
        from_type: MemoryType,
        from_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>> {
        let ids = self
            .by_from
            .get(&(from_type, from_id.to_string()))
            .map(|r| r.value().clone())
            .unwrap_or_default();

        let mut links = self.get_links_by_ids(&ids);

        // Filter by type if specified
        if let Some(lt) = link_type {
            links.retain(|l| l.link_type == lt);
        }

        Ok(links)
    }

    async fn get_links_to(
        &self,
        to_type: MemoryType,
        to_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>> {
        let ids = self
            .by_to
            .get(&(to_type, to_id.to_string()))
            .map(|r| r.value().clone())
            .unwrap_or_default();

        let mut links = self.get_links_by_ids(&ids);

        // Filter by type if specified
        if let Some(lt) = link_type {
            links.retain(|l| l.link_type == lt);
        }

        Ok(links)
    }

    async fn get_links(&self, predicate: &LinkPredicate) -> AdbResult<Vec<Link>> {
        // If we have from_type + from_id, use that index
        if let (Some(from_type), Some(from_id)) = (&predicate.from_type, &predicate.from_id) {
            let ids = self
                .by_from
                .get(&(*from_type, from_id.clone()))
                .map(|r| r.value().clone())
                .unwrap_or_default();

            return Ok(self
                .get_links_by_ids(&ids)
                .into_iter()
                .filter(|l| predicate.matches(l))
                .collect());
        }

        // If we have to_type + to_id, use that index
        if let (Some(to_type), Some(to_id)) = (&predicate.to_type, &predicate.to_id) {
            let ids = self
                .by_to
                .get(&(*to_type, to_id.clone()))
                .map(|r| r.value().clone())
                .unwrap_or_default();

            return Ok(self
                .get_links_by_ids(&ids)
                .into_iter()
                .filter(|l| predicate.matches(l))
                .collect());
        }

        // If we have just link_type, use that index
        if let Some(link_type) = &predicate.link_type {
            let ids = self
                .by_type
                .get(link_type)
                .map(|r| r.value().clone())
                .unwrap_or_default();

            return Ok(self
                .get_links_by_ids(&ids)
                .into_iter()
                .filter(|l| predicate.matches(l))
                .collect());
        }

        // Full scan as fallback
        Ok(self
            .by_id
            .iter()
            .filter(|r| predicate.matches(r.value()))
            .map(|r| r.value().clone())
            .collect())
    }

    async fn update_weight(&self, link_id: &str, weight: f32) -> AdbResult<()> {
        if let Some(mut entry) = self.by_id.get_mut(link_id) {
            entry.set_weight(weight);
        }
        Ok(())
    }

    async fn update_weight_with_signal(
        &self,
        link_id: &str,
        signal: f32,
        decay: f32,
    ) -> AdbResult<()> {
        if let Some(mut entry) = self.by_id.get_mut(link_id) {
            entry.update_weight(signal, decay);
        }
        Ok(())
    }

    async fn forget_links(&self, predicate: &LinkPredicate) -> AdbResult<u64> {
        let links = self.get_links(predicate).await?;
        let count = links.len() as u64;

        for link in links {
            // Remove from indexes
            self.unindex_link(&link);
            // Remove from primary storage
            self.by_id.remove(&link.id);
        }

        self.link_count.fetch_sub(count, Ordering::SeqCst);

        Ok(count)
    }

    async fn count(&self) -> usize {
        self.by_id.len()
    }

    async fn clear(&self) -> AdbResult<()> {
        self.by_id.clear();
        self.by_from.clear();
        self.by_to.clear();
        self.by_type.clear();
        self.link_count.store(0, Ordering::SeqCst);
        Ok(())
    }
}

/// Extension trait for following links to retrieve connected records
#[async_trait]
pub trait FollowLinks: LinkStoreOps {
    /// Follow links from a record and retrieve connected records
    /// Returns (link, record) pairs
    async fn follow_links_from<B: crate::Backend + ?Sized>(
        &self,
        backends: &HashMap<MemoryType, &B>,
        from_type: MemoryType,
        from_id: &str,
        link_type: &str,
        depth: u32,
    ) -> AdbResult<Vec<(Link, MemoryRecord)>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_link() {
        let store = LinkStore::new();

        let link = store
            .link(
                MemoryType::Procedural,
                "oom-fix",
                MemoryType::Episodic,
                "inc-001",
                "applied_to",
                0.95,
            )
            .await
            .unwrap();

        assert_eq!(link.from_type, MemoryType::Procedural);
        assert_eq!(link.from_id, "oom-fix");
        assert_eq!(link.to_type, MemoryType::Episodic);
        assert_eq!(link.to_id, "inc-001");
        assert_eq!(link.link_type, "applied_to");
        assert!((link.weight - 0.95).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_get_links_from() {
        let store = LinkStore::new();

        // Create multiple links from same source
        store
            .link(
                MemoryType::Procedural,
                "pattern-1",
                MemoryType::Episodic,
                "inc-001",
                "applied_to",
                0.9,
            )
            .await
            .unwrap();
        store
            .link(
                MemoryType::Procedural,
                "pattern-1",
                MemoryType::Episodic,
                "inc-002",
                "applied_to",
                0.8,
            )
            .await
            .unwrap();
        store
            .link(
                MemoryType::Procedural,
                "pattern-2",
                MemoryType::Episodic,
                "inc-003",
                "applied_to",
                0.7,
            )
            .await
            .unwrap();

        let links = store
            .get_links_from(MemoryType::Procedural, "pattern-1", None)
            .await
            .unwrap();

        assert_eq!(links.len(), 2);
    }

    #[tokio::test]
    async fn test_get_links_to() {
        let store = LinkStore::new();

        store
            .link(
                MemoryType::Semantic,
                "concept-1",
                MemoryType::Procedural,
                "pattern-1",
                "triggers",
                0.8,
            )
            .await
            .unwrap();
        store
            .link(
                MemoryType::Semantic,
                "concept-2",
                MemoryType::Procedural,
                "pattern-1",
                "triggers",
                0.7,
            )
            .await
            .unwrap();

        let links = store
            .get_links_to(MemoryType::Procedural, "pattern-1", Some("triggers"))
            .await
            .unwrap();

        assert_eq!(links.len(), 2);
    }

    #[tokio::test]
    async fn test_update_weight() {
        let store = LinkStore::new();

        let link = store
            .link(
                MemoryType::Procedural,
                "test",
                MemoryType::Episodic,
                "test",
                "applied_to",
                0.5,
            )
            .await
            .unwrap();

        store.update_weight(&link.id, 0.9).await.unwrap();

        let links = store
            .get_links_from(MemoryType::Procedural, "test", None)
            .await
            .unwrap();

        assert!((links[0].weight - 0.9).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_update_weight_with_signal() {
        let store = LinkStore::new();

        let link = store
            .link(
                MemoryType::Procedural,
                "test",
                MemoryType::Episodic,
                "test",
                "applied_to",
                0.5,
            )
            .await
            .unwrap();

        // Success signal with 0.9 decay
        store
            .update_weight_with_signal(&link.id, 1.0, 0.9)
            .await
            .unwrap();

        let links = store
            .get_links_from(MemoryType::Procedural, "test", None)
            .await
            .unwrap();

        // new = 0.5 * 0.9 + 1.0 * 0.1 = 0.45 + 0.1 = 0.55
        assert!((links[0].weight - 0.55).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_forget_links() {
        let store = LinkStore::new();

        store
            .link(
                MemoryType::Procedural,
                "p1",
                MemoryType::Episodic,
                "e1",
                "applied_to",
                0.9,
            )
            .await
            .unwrap();
        store
            .link(
                MemoryType::Procedural,
                "p1",
                MemoryType::Episodic,
                "e2",
                "applied_to",
                0.8,
            )
            .await
            .unwrap();
        store
            .link(
                MemoryType::Semantic,
                "s1",
                MemoryType::Procedural,
                "p1",
                "triggers",
                0.7,
            )
            .await
            .unwrap();

        assert_eq!(store.count().await, 3);

        // Forget all links from p1
        let pred = LinkPredicate::from_record(MemoryType::Procedural, "p1");
        let count = store.forget_links(&pred).await.unwrap();

        assert_eq!(count, 2);
        assert_eq!(store.count().await, 1);
    }

    #[tokio::test]
    async fn test_type_stats() {
        let store = LinkStore::new();

        store
            .link(MemoryType::Procedural, "p1", MemoryType::Episodic, "e1", "applied_to", 0.9)
            .await
            .unwrap();
        store
            .link(MemoryType::Procedural, "p2", MemoryType::Episodic, "e2", "applied_to", 0.8)
            .await
            .unwrap();
        store
            .link(MemoryType::Semantic, "s1", MemoryType::Procedural, "p1", "triggers", 0.7)
            .await
            .unwrap();

        let stats = store.type_stats();

        assert_eq!(stats.get("applied_to"), Some(&2));
        assert_eq!(stats.get("triggers"), Some(&1));
    }
}
