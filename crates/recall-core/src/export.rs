//! JSON export/import of the whole store (backup and migration, PRD user story 4).

use serde::{Deserialize, Serialize};

use crate::error::{RecallError, Result};
use crate::models::{Memory, Namespace, NewMemory};
use crate::store::Store;

/// On-disk export format. `version` guards future format changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportData {
    pub version: u32,
    pub exported_at: i64,
    pub namespaces: Vec<Namespace>,
    pub memories: Vec<Memory>,
}

pub const EXPORT_VERSION: u32 = 1;

/// Snapshot every namespace and memory. Listing caps are internal-only here
/// (10k scan cap per store contract; larger stores are documented out of v1 scope).
pub fn export(store: &dyn Store) -> Result<ExportData> {
    Ok(ExportData {
        version: EXPORT_VERSION,
        exported_at: crate::unix_now(),
        namespaces: store.list_namespaces()?,
        memories: store.list_memories(None, usize::MAX - 1, 0)?,
    })
}

/// Result of an import run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportReport {
    pub namespaces_created: u64,
    pub memories_inserted: u64,
    /// Memories whose id already existed and were left untouched.
    pub memories_skipped: u64,
}

fn to_new(memory: &Memory, namespace: &str) -> NewMemory {
    NewMemory {
        namespace: namespace.to_string(),
        text: memory.text.clone(),
        tags: memory.tags.clone(),
        source: Some(memory.source.clone()),
        pinned: memory.pinned,
        created_at: Some(memory.created_at),
        id: Some(memory.id.clone()),
        embedding: memory.embedding.clone(),
    }
}

/// Import an export: namespaces are created if missing, memories keep their ids
/// and timestamps; duplicates are skipped, never overwritten.
pub fn import(store: &dyn Store, data: &ExportData) -> Result<ImportReport> {
    if data.version != EXPORT_VERSION {
        return Err(RecallError::InvalidInput(format!(
            "unsupported export version {} (expected {EXPORT_VERSION})",
            data.version
        )));
    }
    let mut report = ImportReport {
        namespaces_created: 0,
        memories_inserted: 0,
        memories_skipped: 0,
    };
    let before = store.list_namespaces()?;
    for ns in &data.namespaces {
        store.get_or_create_namespace(&ns.name)?;
    }
    let after = store.list_namespaces()?;
    report.namespaces_created = (after.len() - before.len()) as u64;

    let ns_by_id: std::collections::HashMap<String, String> = data
        .namespaces
        .iter()
        .map(|ns| (ns.id.clone(), ns.name.clone()))
        .collect();
    for memory in &data.memories {
        let namespace = match ns_by_id.get(&memory.namespace_id) {
            Some(name) => name.clone(),
            None => {
                return Err(RecallError::InvalidInput(format!(
                    "export references unknown namespace id {}",
                    memory.namespace_id
                )));
            }
        };
        if store.get_memory(&memory.id)?.is_some() {
            report.memories_skipped += 1;
            continue;
        }
        store.insert_memory(&to_new(memory, &namespace))?;
        report.memories_inserted += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{Embedder as _, HashEmbedder};
    use crate::sqlite::SqliteStore;

    fn seeded() -> SqliteStore {
        let s = SqliteStore::open_in_memory().unwrap();
        let w = s.create_namespace("work").unwrap();
        s.insert_memory(&NewMemory {
            namespace: "work".into(),
            text: "rust test memory".into(),
            tags: vec!["t".into()],
            source: Some("human".into()),
            pinned: true,
            created_at: Some(1_234),
            id: Some("mem-1".into()),
            embedding: Some(HashEmbedder::new_256().embed("rust test memory")),
        })
        .unwrap();
        s.get_or_create_namespace("empty").unwrap();
        let _ = w;
        s
    }

    #[test]
    fn export_roundtrip_into_fresh_store_preserves_everything() {
        let src = seeded();
        let data = export(&src).unwrap();
        assert_eq!(data.version, 1);
        assert_eq!(data.namespaces.len(), 2);
        assert_eq!(data.memories.len(), 1);

        // The export itself must be valid JSON (the format backup users rely on).
        let json = serde_json::to_string(&data).unwrap();
        let parsed: ExportData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, data);

        let dst = SqliteStore::open_in_memory().unwrap();
        let report = import(&dst, &parsed).unwrap();
        assert_eq!(report.namespaces_created, 2);
        assert_eq!(report.memories_inserted, 1);
        assert_eq!(report.memories_skipped, 0);

        let restored = dst.get_memory("mem-1").unwrap().unwrap();
        assert_eq!(restored.text, "rust test memory");
        assert_eq!(restored.created_at, 1_234);
        assert!(restored.pinned);
        assert!(restored.embedding.is_some());
        assert!(dst.find_namespace("empty").unwrap().is_some());
    }

    #[test]
    fn import_is_idempotent_by_skipping_existing_ids() {
        let src = seeded();
        let data = export(&src).unwrap();
        let report = import(&src, &data).unwrap();
        assert_eq!(report.memories_inserted, 0);
        assert_eq!(report.memories_skipped, 1);
        assert_eq!(report.namespaces_created, 0);
        assert_eq!(src.stats().unwrap().total_memories, 1);
    }

    #[test]
    fn import_rejects_unknown_versions_and_dangling_namespace_ids() {
        let dst = SqliteStore::open_in_memory().unwrap();
        let mut bad = ExportData {
            version: 99,
            exported_at: 0,
            namespaces: vec![],
            memories: vec![],
        };
        assert!(import(&dst, &bad).is_err());

        bad.version = 1;
        bad.memories.push(Memory {
            id: "m".into(),
            namespace_id: "ghost".into(),
            text: "x".into(),
            tags: vec![],
            embedding: None,
            source: "agent".into(),
            pinned: false,
            created_at: 0,
            last_accessed_at: 0,
        });
        let err = import(&dst, &bad).unwrap_err();
        assert!(err.to_string().contains("unknown namespace id"));
    }

    #[test]
    fn export_of_empty_store_is_empty_and_importable() {
        let empty = SqliteStore::open_in_memory().unwrap();
        let data = export(&empty).unwrap();
        assert!(data.namespaces.is_empty() && data.memories.is_empty());
        let report = import(&SqliteStore::open_in_memory().unwrap(), &data).unwrap();
        assert_eq!(
            report,
            ImportReport {
                namespaces_created: 0,
                memories_inserted: 0,
                memories_skipped: 0
            }
        );
    }
}
