//! Stateless extraction only. Callers own authorization, tenancy, persistence and actions.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractRequest {
    pub file_name: String,
    pub media_type: String,
    pub content_base64: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType { Invoice, Receipt, MeterReading, Other }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentFields {
    pub invoice_number: Option<String>,
    pub issue_date: Option<String>,
    pub currency: Option<String>,
    pub total_amount: Option<String>,
    pub supplier_name: Option<String>,
    pub supplier_registration_number: Option<String>,
    pub meter_number: Option<String>,
    pub reading_value: Option<String>,
    pub reading_date: Option<String>,
    pub property_address: Option<String>,
    pub unit_number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldEvidence { pub field: String, pub page: u32, pub quote: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub document_type: DocumentType,
    pub fields: DocumentFields,
    pub evidence: Vec<FieldEvidence>,
    pub warnings: Vec<String>,
    pub model: String,
}

#[derive(thiserror::Error, Debug)]
pub enum DocumentError {
    #[error("documents.invalid_input")]
    InvalidInput,
    #[error("documents.invalid_config")]
    InvalidConfig,
    #[error("documents.transport")]
    Transport,
    #[error("documents.http_status: {0}")]
    HttpStatus(u16),
    #[error("documents.response_too_large")]
    ResponseTooLarge,
    #[error("documents.invalid_output")]
    InvalidOutput,
}

#[async_trait]
pub trait VisionProvider: Send + Sync {
    async fn extract(&self, request: ExtractRequest) -> Result<Extraction, DocumentError>;
}

/// Server-owned configuration. Never construct this from upload metadata.
#[derive(Clone)]
pub struct VisionConfig { pub base_url: String, pub model: String, pub api_key: String }

pub struct OpenAiVisionProvider;
impl OpenAiVisionProvider {
    pub fn new(_config: VisionConfig) -> Result<Self, DocumentError> { Ok(Self) }
    pub fn with_timeout(_config: VisionConfig, _timeout: Duration) -> Result<Self, DocumentError> { Ok(Self) }
}
#[async_trait]
impl VisionProvider for OpenAiVisionProvider {
    async fn extract(&self, _request: ExtractRequest) -> Result<Extraction, DocumentError> { Err(DocumentError::InvalidOutput) }
}

pub fn validate_request(_request: &ExtractRequest) -> Result<(), DocumentError> { Ok(()) }
pub fn validate_extraction(_extraction: &Extraction) -> Result<(), DocumentError> { Ok(()) }
