//! Token-efficient response views shared by MCP, REST, and the CLI.
//!
//! The full [`Memory`] record carries the embedding vector, `namespace_id`,
//! `pinned`, and `last_accessed_at`, which are useful for storage work and
//! expensive in an agent's context. These views are the single compact
//! serialization layer every surface uses, so the three shells cannot drift.
//! `include_embedding` opts back into the full record.

use serde::{Deserialize, Serialize};

use crate::models::{Memory, RecallHit, ScoreBreakdown};

/// Compact memory: the six fields a caller needs to use and cite a record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompactMemory {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: i64,
}

impl From<&Memory> for CompactMemory {
    fn from(memory: &Memory) -> Self {
        Self {
            id: memory.id.clone(),
            text: memory.text.clone(),
            tags: memory.tags.clone(),
            source: memory.source.clone(),
            created_at: memory.created_at,
        }
    }
}

/// Compact recall hit: compact memory plus the existing score breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompactRecallHit {
    #[serde(flatten)]
    pub memory: CompactMemory,
    pub breakdown: ScoreBreakdown,
}

impl From<&RecallHit> for CompactRecallHit {
    fn from(hit: &RecallHit) -> Self {
        Self {
            memory: CompactMemory::from(&hit.memory),
            breakdown: hit.breakdown.clone(),
        }
    }
}

/// Compact page: enough to paginate without dumping the namespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompactListPage<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}

impl<T> CompactListPage<T> {
    pub fn new(items: Vec<T>, total: u64, limit: usize, offset: usize) -> Self {
        let has_more = (offset as u64).saturating_add(items.len() as u64) < total;
        Self {
            items,
            total,
            limit,
            offset,
            has_more,
        }
    }
}

/// Compact capture summary: what landed, without echoing every chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompactCaptureSummary {
    pub namespace: String,
    pub captured: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

impl CompactCaptureSummary {
    pub fn new(namespace: impl Into<String>, memories: &[Memory]) -> Self {
        Self {
            namespace: namespace.into(),
            captured: memories.len(),
            first_id: memories.first().map(|m| m.id.clone()),
            last_id: memories.last().map(|m| m.id.clone()),
        }
    }
}

/// Compact text rendering for `format=text` recall. Preserves rank order,
/// each hit's full score breakdown, and the memory text; one block per hit.
pub fn render_hits_text(hits: &[RecallHit]) -> String {
    if hits.is_empty() {
        return "no matches\n".to_string();
    }
    let mut out = String::new();
    for (index, hit) in hits.iter().enumerate() {
        let b = &hit.breakdown;
        out.push_str(&format!(
            "{}. [{:.3}] id={} created_at={} source={} tags={}\n   {}\n   \
             bm25={:.3} vector={:.3} recency={:.3} pinned_boost={:.3}\n",
            index + 1,
            b.total,
            hit.memory.id,
            hit.memory.created_at,
            hit.memory.source,
            if hit.memory.tags.is_empty() {
                "-".to_string()
            } else {
                hit.memory.tags.join(",")
            },
            hit.memory.text,
            b.bm25,
            b.vector,
            b.recency,
            b.pinned_boost,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Memory {
        Memory {
            id: "m-1".into(),
            namespace_id: "ns-1".into(),
            text: "deploy on friday".into(),
            tags: vec!["ops".into()],
            embedding: Some(vec![0.1, 0.2]),
            source: "cli".into(),
            pinned: true,
            created_at: 1_786_000_000,
            last_accessed_at: 1_786_000_000,
        }
    }

    #[test]
    fn compact_memory_drops_storage_details_and_empty_tags() {
        let compact = CompactMemory::from(&memory());
        let json = serde_json::to_value(&compact).unwrap();
        assert_eq!(json["id"], "m-1");
        assert_eq!(json["text"], "deploy on friday");
        assert_eq!(json["tags"][0], "ops");
        assert!(json.get("embedding").is_none());
        assert!(json.get("namespace_id").is_none());
        assert!(json.get("pinned").is_none());
        assert!(json.get("last_accessed_at").is_none());
        // Empty tag lists are omitted entirely.
        let untagged = CompactMemory::from(&Memory {
            tags: vec![],
            ..memory()
        });
        assert!(
            !serde_json::to_value(untagged)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("tags")
        );
    }

    #[test]
    fn compact_hit_keeps_the_breakdown() {
        let hit = RecallHit {
            memory: memory(),
            breakdown: ScoreBreakdown {
                bm25: 1.0,
                vector: 0.5,
                recency: 1.0,
                pinned_boost: 0.2,
                total: 0.875,
            },
        };
        let compact = CompactRecallHit::from(&hit);
        let json = serde_json::to_value(&compact).unwrap();
        assert_eq!(json["breakdown"]["total"], 0.875);
        assert_eq!(json["id"], "m-1");
    }

    #[test]
    fn list_page_reports_has_more() {
        let page = CompactListPage::new(vec![1, 2, 3], 10, 3, 0);
        assert!(page.has_more);
        let last = CompactListPage::new(vec![1], 4, 3, 3);
        assert!(!last.has_more);
    }

    #[test]
    fn capture_summary_names_first_and_last_ids() {
        let a = Memory {
            id: "a".into(),
            ..memory()
        };
        let b = Memory {
            id: "b".into(),
            ..memory()
        };
        let summary = CompactCaptureSummary::new("session-1", &[a, b]);
        assert_eq!(summary.captured, 2);
        assert_eq!(summary.first_id.as_deref(), Some("a"));
        assert_eq!(summary.last_id.as_deref(), Some("b"));
        let empty = CompactCaptureSummary::new("session-1", &[]);
        assert_eq!(empty.first_id, None);
    }

    #[test]
    fn text_rendering_preserves_ranking_and_breakdown() {
        let hit = RecallHit {
            memory: memory(),
            breakdown: ScoreBreakdown {
                bm25: 1.0,
                vector: 0.5,
                recency: 1.0,
                pinned_boost: 0.2,
                total: 0.875,
            },
        };
        let text = render_hits_text(&[hit.clone(), hit]);
        assert!(text.starts_with("1. [0.875]"));
        assert!(text.contains("2. [0.875]"));
        assert!(text.contains("bm25=1.000 vector=0.500 recency=1.000 pinned_boost=0.200"));
        assert!(text.contains("deploy on friday"));
        assert_eq!(render_hits_text(&[]), "no matches\n");
    }
}
