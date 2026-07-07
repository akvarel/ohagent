//! Speed benchmark — measures real-world latency and throughput per provider.

use chrono::Utc;
use std::time::Instant;
use crate::models::SpeedRecord;

pub struct SpeedBenchmark {
    client: reqwest::Client,
}

/// A benchmark config for a specific provider+model.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub provider: String,
    pub model_id: String,
    pub api_key: String,
    pub api_base: String,
    pub samples: u32,
}

impl SpeedBenchmark {
    pub fn new() -> Self {
        Self { client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default()
        }
    }

    /// Run a speed benchmark for one provider+model.
    pub async fn run(&self, config: &BenchmarkConfig) -> SpeedRecord {
        let prompt = "Explain what a binary tree is in one paragraph. Be concise.";
        let body = serde_json::json!({
            "model": config.model_id,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 500,
            "stream": true,
        });

        let mut ttf_samples: Vec<u64> = Vec::new();
        let mut total_latency_samples: Vec<u64> = Vec::new();
        let mut tps_samples: Vec<f64> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for _ in 0..config.samples {
            let start = Instant::now();
            let mut first_token_time: Option<u64> = None;
            let mut token_count: u32 = 0;

            let result = self.client
                .post(&format!("{}/chat/completions", config.api_base))
                .header("Authorization", format!("Bearer {}", config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status().as_u16();
                        let err_text = response.text().await.unwrap_or_default();
                        errors.push(format!("HTTP {status}: {err_text}"));
                        continue;
                    }

                    // Read streaming response
                    let mut stream = response.bytes_stream();
                    use futures::StreamExt;
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                if first_token_time.is_none() {
                                    first_token_time = Some(start.elapsed().as_millis() as u64);
                                }
                                token_count += 1;
                            }
                            Err(e) => {
                                errors.push(format!("Stream error: {e}"));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("Request error: {e}"));
                }
            }

            let elapsed = start.elapsed();
            if let Some(ft) = first_token_time {
                ttf_samples.push(ft);
                total_latency_samples.push(elapsed.as_millis() as u64);
                let tps = if elapsed.as_secs_f64() > 0.0 {
                    token_count as f64 / elapsed.as_secs_f64()
                } else { 0.0 };
                tps_samples.push(tps);
            }
        }

        let samples = ttf_samples.len() as u32;

        SpeedRecord {
            id: format!("bench:{}:{}:{}", config.provider, config.model_id.replace('/', "_"), Utc::now().timestamp()),
            provider: config.provider.clone(),
            model_id: config.model_id.clone(),
            ttf_ms: median(&ttf_samples),
            total_latency_ms: median(&total_latency_samples),
            tokens_per_second: avg(&tps_samples),
            p95_latency_ms: p95(&total_latency_samples),
            prompt_tokens: prompt.split_whitespace().count() as u32,
            completion_tokens: 500,
            samples,
            measured_at: Utc::now(),
            error: if errors.is_empty() { None } else { Some(errors.join("; ")) },
        }
    }
}

fn median(v: &[u64]) -> u64 {
    if v.is_empty() { return 0; }
    let mut sorted: Vec<u64> = v.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn avg(v: &[f64]) -> f64 {
    if v.is_empty() { return 0.0; }
    v.iter().sum::<f64>() / v.len() as f64
}

fn p95(v: &[u64]) -> u64 {
    if v.is_empty() { return 0; }
    let mut sorted: Vec<u64> = v.to_vec();
    sorted.sort();
    let idx = ((sorted.len() as f64) * 0.95) as usize;
    sorted[idx.min(sorted.len() - 1)]
}
