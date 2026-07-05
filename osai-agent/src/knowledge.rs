// =============================================================================
// File: src/knowledge.rs
// Purpose:
//   Loads static Markdown knowledge files embedded into the binary.
//
// Where this fits in OSAI:
//   Supplies policy, runbooks, identity, and response guidance to reasoning/Ask OSAI flows.
//
// Topics to know before editing:
//   Rust ownership, async/await, serde data models, error handling, and this project's scan/memory/ask flow.
//
// Important operational notes:
//   Knowledge files are source-controlled context; do not put secrets or host-specific private data there.
// =============================================================================
// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

use std::{collections::BTreeMap, fs, path::Path};

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct KnowledgeBase {
    files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeMatch {
    pub file: String,
    pub score: usize,
    pub excerpt: String,
}

impl KnowledgeBase {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut files = BTreeMap::new();

        if !path.exists() {
            return Ok(Self { files });
        }

        // Load only Markdown runbooks. This keeps the local knowledge layer
        // transparent and easy to edit without introducing a database dependency.
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();

            if file_path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }

            let Some(name) = file_path.file_name().and_then(|x| x.to_str()) else {
                continue;
            };

            let content = fs::read_to_string(&file_path)?;
            files.insert(name.to_string(), content);
        }

        Ok(Self { files })
    }

    pub fn list(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.files.get(name).cloned()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<KnowledgeMatch> {
        let terms = normalize_terms(query);
        if terms.is_empty() {
            return Vec::new();
        }

        // Simple term overlap is enough for local runbook hints. Long-term
        // semantic memory belongs in Cognee/pgvector, not this lightweight loader.
        let mut matches = self
            .files
            .iter()
            .filter_map(|(name, content)| {
                let lower = content.to_lowercase();
                let score = terms.iter().filter(|term| lower.contains(term.as_str())).count();
                if score == 0 {
                    return None;
                }
                Some(KnowledgeMatch {
                    file: name.clone(),
                    score,
                    excerpt: best_excerpt(content, &terms),
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|a, b| b.score.cmp(&a.score).then(a.file.cmp(&b.file)));
        matches.truncate(limit.max(1));
        matches
    }
}

fn normalize_terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .map(|term| term.trim().to_lowercase())
        .filter(|term| term.len() >= 3)
        .collect()
}

fn best_excerpt(content: &str, terms: &[String]) -> String {
    let lower = content.to_lowercase();
    let first_hit = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0);

    let start = first_hit.saturating_sub(180);
    let end = (first_hit + 420).min(content.len());

    content
        .get(start..end)
        .unwrap_or(content)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
