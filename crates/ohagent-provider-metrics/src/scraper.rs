//! Price scraper — loads known validated prices from discovery module.

use crate::discovery::known_prices;
use crate::store::MetricsStore;

pub struct PriceScraper;

impl PriceScraper {
    pub fn new() -> Self { Self }
    pub async fn scrape_all(&self, store: &MetricsStore) -> Result<usize, String> {
        let prices = known_prices();
        let count = prices.len();
        for record in prices {
            store.upsert_price(&record)?;
        }
        tracing::info!(%count, "Price scrape completed");
        Ok(count)
    }
}
