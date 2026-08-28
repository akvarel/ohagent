//! Billing tracker — per-key usage, markup calculation, invoices.

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Mutex;

use crate::apikeys::MarkupTier;

#[derive(Debug, Clone, Serialize)]
pub struct InvoiceSummary {
    pub customer_id: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_our_cost: f64,
    pub total_customer_cost: f64,
    pub total_margin: f64,
    pub by_provider: Vec<ProviderSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub provider: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub our_cost: f64,
    pub customer_cost: f64,
    pub margin: f64,
}

pub struct BillingTracker {
    db: Arc<Mutex<rusqlite::Connection>>,
}

impl BillingTracker {
    pub fn new(db: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { db }
    }

    pub fn record(
        &self,
        api_key_id: &str,
        customer_id: &str,
        provider: &str,
        model_id: &str,
        prompt: u64,
        completion: u64,
        our_cost: f64,
        tier: &MarkupTier,
    ) -> Result<f64, String> {
        let markup = tier.markup();
        let customer_cost = our_cost * markup;
        let margin = customer_cost - our_cost;
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        db.execute(
            "INSERT INTO usage_records (id,api_key_id,customer_id,provider,model_id,prompt_tokens,completion_tokens,our_cost_eur,customer_cost_eur,margin_eur,timestamp) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![uuid::Uuid::new_v4().to_string(), api_key_id, customer_id, provider, model_id, prompt, completion, our_cost, customer_cost, margin, Utc::now().to_rfc3339()],
        ).map_err(|e| format!("insert: {e}"))?;
        Ok(customer_cost)
    }

    pub fn check_quota(&self, api_key_id: &str, limit: u64) -> Result<bool, String> {
        if limit == 0 {
            return Ok(true);
        }
        let today = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let used: u64 = db.query_row("SELECT COALESCE(SUM(prompt_tokens+completion_tokens),0) FROM usage_records WHERE api_key_id=?1 AND timestamp>=?2",
            params![api_key_id, today.to_rfc3339()], |r| r.get(0)).unwrap_or(0);
        Ok(used < limit)
    }

    pub fn invoice(
        &self,
        customer_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<InvoiceSummary, String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let mut s = db.prepare("SELECT COALESCE(SUM(prompt_tokens),0),COALESCE(SUM(completion_tokens),0),COALESCE(SUM(our_cost_eur),0),COALESCE(SUM(customer_cost_eur),0),COALESCE(SUM(margin_eur),0) FROM usage_records WHERE customer_id=?1 AND timestamp BETWEEN ?2 AND ?3").map_err(|e| format!("prep: {e}"))?;
        let (tp, tc, to, tcc, tm): (u64, u64, f64, f64, f64) = s
            .query_row(
                params![customer_id, start.to_rfc3339(), end.to_rfc3339()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap_or((0, 0, 0., 0., 0.));
        let mut ps = db.prepare("SELECT provider,COALESCE(SUM(prompt_tokens),0),COALESCE(SUM(completion_tokens),0),COALESCE(SUM(our_cost_eur),0),COALESCE(SUM(customer_cost_eur),0),COALESCE(SUM(margin_eur),0) FROM usage_records WHERE customer_id=?1 AND timestamp BETWEEN ?2 AND ?3 GROUP BY provider").map_err(|e| format!("prep: {e}"))?;
        let by: Vec<ProviderSummary> = ps
            .query_map(
                params![customer_id, start.to_rfc3339(), end.to_rfc3339()],
                |r| {
                    Ok(ProviderSummary {
                        provider: r.get(0)?,
                        prompt_tokens: r.get(1)?,
                        completion_tokens: r.get(2)?,
                        our_cost: r.get(3)?,
                        customer_cost: r.get(4)?,
                        margin: r.get(5)?,
                    })
                },
            )
            .map_err(|e| format!("q: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("c: {e}"))?;
        Ok(InvoiceSummary {
            customer_id: customer_id.into(),
            period_start: start,
            period_end: end,
            total_prompt_tokens: tp,
            total_completion_tokens: tc,
            total_our_cost: to,
            total_customer_cost: tcc,
            total_margin: tm,
            by_provider: by,
        })
    }
}
