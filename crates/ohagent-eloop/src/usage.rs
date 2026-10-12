use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_missed_tokens: u64,
    pub input_cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: Option<f64>,
}

impl ModelUsage {
    pub fn total_input_tokens(&self) -> u64 {
        self.input_missed_tokens + self.input_cached_tokens + self.cache_write_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens() + self.output_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageTotals {
    pub requests: u64,
    pub input_missed_tokens: u64,
    pub input_cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageReport {
    pub total: UsageTotals,
    pub by_model: Vec<ModelUsage>,
}

impl UsageReport {
    pub fn from_calls(calls: impl IntoIterator<Item = ModelUsage>) -> Self {
        let mut grouped: BTreeMap<(String, String), ModelUsage> = BTreeMap::new();
        for call in calls {
            let key = (call.provider.clone(), call.model.clone());
            let entry = grouped.entry(key).or_insert_with(|| ModelUsage {
                provider: call.provider.clone(),
                model: call.model.clone(),
                ..ModelUsage::default()
            });
            entry.requests += call.requests;
            entry.input_missed_tokens += call.input_missed_tokens;
            entry.input_cached_tokens += call.input_cached_tokens;
            entry.cache_write_tokens += call.cache_write_tokens;
            entry.output_tokens += call.output_tokens;
            entry.estimated_cost_usd = sum_optional(
                entry.estimated_cost_usd,
                call.estimated_cost_usd,
            );
        }

        let by_model = grouped.into_values().collect::<Vec<_>>();
        let total = by_model.iter().fold(UsageTotals::default(), |mut total, usage| {
            total.requests += usage.requests;
            total.input_missed_tokens += usage.input_missed_tokens;
            total.input_cached_tokens += usage.input_cached_tokens;
            total.cache_write_tokens += usage.cache_write_tokens;
            total.output_tokens += usage.output_tokens;
            total.estimated_cost_usd =
                sum_optional(total.estimated_cost_usd, usage.estimated_cost_usd);
            total
        });

        Self { total, by_model }
    }

    pub fn render_markdown(&self) -> String {
        let mut output = String::from(
            "# Model usage\n\n| Provider | Model | Requests | Input missed | Input cached | Cache write | Output | Cost USD |\n|---|---|---:|---:|---:|---:|---:|---:|\n",
        );
        for usage in &self.by_model {
            output.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                usage.provider,
                usage.model,
                usage.requests,
                usage.input_missed_tokens,
                usage.input_cached_tokens,
                usage.cache_write_tokens,
                usage.output_tokens,
                format_cost(usage.estimated_cost_usd),
            ));
        }
        output.push_str(&format!(
            "| **TOTAL** |  | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |\n",
            self.total.requests,
            self.total.input_missed_tokens,
            self.total.input_cached_tokens,
            self.total.cache_write_tokens,
            self.total.output_tokens,
            format_cost(self.total.estimated_cost_usd),
        ));
        output
    }
}

fn sum_optional(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or_default() + right.unwrap_or_default()),
    }
}

fn format_cost(cost: Option<f64>) -> String {
    cost.map(|value| format!("{value:.6}")).unwrap_or_else(|| "n/a".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_total_and_by_model() {
        let report = UsageReport::from_calls([
            ModelUsage {
                provider: "openai".into(),
                model: "gpt".into(),
                requests: 1,
                input_missed_tokens: 10,
                input_cached_tokens: 20,
                output_tokens: 5,
                ..ModelUsage::default()
            },
            ModelUsage {
                provider: "openai".into(),
                model: "gpt".into(),
                requests: 1,
                input_missed_tokens: 2,
                input_cached_tokens: 3,
                output_tokens: 4,
                ..ModelUsage::default()
            },
        ]);
        assert_eq!(report.by_model.len(), 1);
        assert_eq!(report.total.requests, 2);
        assert_eq!(report.total.input_missed_tokens, 12);
        assert_eq!(report.total.input_cached_tokens, 23);
        assert_eq!(report.total.output_tokens, 9);
    }
}
