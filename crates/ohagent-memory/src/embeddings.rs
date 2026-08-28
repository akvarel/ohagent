//! Embedding generator — wraps Jcode's ONNX-based embedder.
//!
//! Uses `jcode_base::embedding` for local, offline embeddings.
//! Behind `embeddings` feature flag to avoid heavy ONNX dependencies in tests.

use std::sync::Arc;

use crate::models::MemoryEntry;
use crate::Result;

#[cfg(feature = "embeddings")]
use tracing::{debug, warn};

/// Generate an embedding for a memory entry's content.
///
/// When the `embeddings` feature is enabled, uses Jcode's ONNX embedder.
/// Otherwise, returns without embedding.
pub fn embed_entry(entry: &mut MemoryEntry) -> Result<()> {
    let _ = entry;
    #[cfg(feature = "embeddings")]
    {
        if !jcode_base::embedding::is_model_available() {
            warn!("Jcode embedding model not available — skipping embedding");
            return Ok(());
        }
        match embed_text(&entry.content) {
            Ok(vec) => {
                debug!(id = %entry.id, dims = vec.len(), "Entry embedded");
                entry.embedding = Some(vec);
            }
            Err(e) => {
                warn!(id = %entry.id, error = %e, "Embedding failed, storing without vector");
            }
        }
    }
    Ok(())
}

/// Generate an embedding for arbitrary text.
#[cfg(feature = "embeddings")]
pub fn embed_text(text: &str) -> Result<Vec<f32>> {
    if text.trim().is_empty() {
        return Ok(vec![0.0f32; 384]);
    }
    if !jcode_base::embedding::is_model_available() {
        return Err(
            "Jcode embedding model not available. Run Jcode once to download ONNX model.".into(),
        );
    }
    let vec = jcode_base::embedding::embed(text)?;
    Ok(vec)
}

/// Generate an embedding for arbitrary text (no-op without embeddings feature).
#[cfg(not(feature = "embeddings"))]
pub fn embed_text(_text: &str) -> Result<Vec<f32>> {
    Err("Embeddings feature not enabled".into())
}

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    #[cfg(feature = "embeddings")]
    {
        return jcode_base::embedding::cosine_similarity(a, b);
    }
    #[cfg(not(feature = "embeddings"))]
    {
        // Fallback implementation
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}

/// Find most similar entries given a query embedding and candidate embeddings.
pub fn find_similar(
    query: &[f32],
    candidates: &[Arc<(MemoryEntry, Vec<f32>)>],
    top_k: usize,
    threshold: f32,
) -> Vec<(MemoryEntry, f32)> {
    if candidates.is_empty() || query.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f32)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, cosine_similarity(query, &c.1)))
        .filter(|(_, s)| *s >= threshold)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(idx, score)| (candidates[idx].0.clone(), score))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0f32, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.001);
    }
}
