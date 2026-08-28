//! S3 archive — monthly upload of message logs to S3 with Glacier tiering.
//!
//! Uses the `aws` CLI for reliable S3 upload with `--storage-class` flag.
//! This avoids pulling in the heavyweight AWS SDK.
//!
//! ## Prerequisites
//!
//! - `awscli` installed (`pip install awscli` or `apt install awscli`)
//! - AWS credentials in env (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
//! - Or IAM role on EC2/EKS
//!
//! ## Configuration
//!
//! Environment variables or keys in `~/.ohagent/keys.toml`:
//! - `S3_BUCKET` — bucket name (e.g. `my-logs-bucket`)
//! - `S3_REGION` — AWS region (default: `us-east-1`)
//! - `S3_ARCHIVE_AFTER_DAYS` — days before archiving (default: 30)
//! - `S3_STORAGE_CLASS` — Glacier tier (default: `GLACIER_IR`)

use std::io::Write;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::message_log::MessageLog;

/// S3 archive configuration.
#[derive(Debug, Clone)]
pub struct S3ArchiveConfig {
    pub bucket: String,
    pub region: String,
    /// Days before archiving (uploaded to S3, then local records marked archived)
    pub archive_after_days: i32,
    /// Storage class: "GLACIER_IR" (instant retrieval) or "DEEP_ARCHIVE"
    pub storage_class: String,
}

impl Default for S3ArchiveConfig {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            region: "us-east-1".into(),
            archive_after_days: 30,
            storage_class: "GLACIER_IR".into(),
        }
    }
}

impl S3ArchiveConfig {
    /// Load from environment variables.
    pub fn from_env() -> Option<Self> {
        let bucket = std::env::var("S3_BUCKET").ok()?;
        Some(Self {
            bucket,
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
            archive_after_days: std::env::var("S3_ARCHIVE_AFTER_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            storage_class: std::env::var("S3_STORAGE_CLASS")
                .unwrap_or_else(|_| "GLACIER_IR".into()),
        })
    }

    /// Whether S3 is configured.
    pub fn is_configured(&self) -> bool {
        !self.bucket.is_empty()
    }
}

/// Handles monthly S3 archival using `aws s3 cp`.
pub struct S3Archive {
    config: S3ArchiveConfig,
    message_log: Arc<MessageLog>,
}

impl S3Archive {
    pub fn new(config: S3ArchiveConfig, message_log: Arc<MessageLog>) -> Self {
        Self {
            config,
            message_log,
        }
    }

    /// Run a single archival cycle:
    /// 1. Dump old unarchived entries to a gzipped JSON file
    /// 2. Upload to S3 with Glacier storage class
    /// 3. Mark entries as archived in DB
    /// 4. Clean up local file
    pub async fn run_cycle(&self) -> Result<ArchiveResult> {
        if !self.config.is_configured() {
            tracing::debug!("S3 not configured, skipping archive cycle");
            return Ok(ArchiveResult::skipped());
        }

        let entries = self
            .message_log
            .ready_for_archive(self.config.archive_after_days)?;

        if entries.is_empty() {
            return Ok(ArchiveResult {
                uploaded: 0,
                bytes: 0,
                skipped: true,
            });
        }

        // Serialize to NDJSON + gzip
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let local_path = format!("/tmp/ohagent-archive-{}.json.gz", date);
        let s3_key = format!(
            "ohagent-logs/{}/{}",
            date,
            local_path.split('/').last().unwrap()
        );

        {
            let file = std::fs::File::create(&local_path)
                .with_context(|| format!("Failed to create {local_path}"))?;
            let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::best());
            for entry in &entries {
                // Write as NDJSON: one JSON object per line
                let line = serde_json::json!({
                    "id": entry.id,
                    "tenant_id": entry.tenant_id,
                    "session_hash": entry.session_hash,
                    "role": entry.role,
                    "turn_seq": entry.turn_seq,
                    "content": entry.content_json,
                    "token_estimate": entry.token_estimate,
                    "created_at": entry.created_at,
                });
                writeln!(gz, "{}", serde_json::to_string(&line)?)?;
            }
            gz.finish()?;
        }

        let file_size = std::fs::metadata(&local_path).map(|m| m.len()).unwrap_or(0);

        // Upload via awscli
        let s3_uri = format!("s3://{}/{}", self.config.bucket, s3_key);
        tracing::info!(
            local = %local_path,
            s3 = %s3_uri,
            entries = entries.len(),
            bytes = file_size,
            "Uploading archive to S3"
        );

        let output = Command::new("aws")
            .args([
                "s3",
                "cp",
                &local_path,
                &s3_uri,
                "--storage-class",
                &self.config.storage_class,
                "--region",
                &self.config.region,
            ])
            .output()
            .context("Failed to run `aws s3 cp`. Is awscli installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("aws s3 cp failed: {}", stderr));
        }

        // Mark as archived in DB
        let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
        let count = ids.len();
        self.message_log.mark_archived(&ids)?;

        // Clean up local file
        let _ = std::fs::remove_file(&local_path);

        // Also cleanup old archived records from DB
        let _ = self
            .message_log
            .cleanup_archived(self.config.archive_after_days + 7);

        tracing::info!(
            uploaded = count,
            bytes = file_size,
            "S3 archive cycle complete"
        );

        Ok(ArchiveResult {
            uploaded: count as u64,
            bytes: file_size,
            skipped: false,
        })
    }
}

/// Result of an archive cycle.
#[derive(Debug)]
pub struct ArchiveResult {
    pub uploaded: u64,
    pub bytes: u64,
    pub skipped: bool,
}

impl ArchiveResult {
    fn skipped() -> Self {
        Self {
            uploaded: 0,
            bytes: 0,
            skipped: true,
        }
    }
}
