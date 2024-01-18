use async_graphql::SimpleObject;
use futures::future::Map;
use sqlx::SqlitePool;

#[derive(SimpleObject)]
pub struct Currency {
    pub id: String,
    pub display_name: String,
    pub rate: f64,
    pub symbol: String,
    pub decimals: i64,
}

impl Currency {
    pub async fn get_all(pool: &SqlitePool) -> anyhow::Result<Vec<Self>> {
        let currencies = sqlx::query_as!(Currency, "SELECT * FROM currency")
            .fetch_all(pool)
            .await?;
        Ok(currencies)
    }
    pub async fn get_for_id(pool: &SqlitePool, id: &str) -> anyhow::Result<Self> {
        let currencies = sqlx::query_as!(Currency, "SELECT * FROM currency WHERE id = $1", id)
            .fetch_one(pool)
            .await?;
        Ok(currencies)
    }
}

use serde::Deserialize;

use crate::REQWEST_CLIENT;

#[derive(Debug, Deserialize)]
struct FreeCurrency {
    symbol: String,
    name: String,
    symbol_native: String,
    decimal_digits: u32,
    rounding: u32,
    code: String,
    name_plural: String,
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct CurrencyData {
    #[serde(flatten)]
    currencies: std::collections::BTreeMap<String, FreeCurrency>,
}

#[derive(Debug, Deserialize)]
struct CurrencyResponse {
    data: CurrencyData,
}

#[derive(Debug, Deserialize)]
struct PricesResponse {
    data: std::collections::BTreeMap<String, f64>,
}

//Free currency api
impl Currency {
    pub async fn fill_currencies(pool: &SqlitePool) -> anyhow::Result<()> {
        let freecurrency_token = std::env::var("FREE_CURRENCY_TOKEN").unwrap();
        let mut currencies = REQWEST_CLIENT
            .get(format!(
                "https://api.freecurrencyapi.com/v1/currencies?apikey={freecurrency_token}"
            ))
            .send()
            .await?
            .json::<CurrencyResponse>()
            .await?;
        if let Some(entry) = currencies.data.currencies.get_mut("INR") {
            entry.symbol = "₹".to_string();
        }
        let prices = REQWEST_CLIENT
            .get(format!(
                "https://api.freecurrencyapi.com/v1/latest?apikey={freecurrency_token}&currencies="
            ))
            .send()
            .await?
            .json::<PricesResponse>()
            .await?;
        let mut transaction = pool.begin().await?;
        for (code, currency) in currencies.data.currencies {
            sqlx::query!(
                "
            INSERT OR REPLACE INTO currency(id, display_name, symbol,rate,decimals)
            VALUES ($1,$2,$3,$4,$5)
            ",
                currency.code,
                currency.name,
                currency.symbol,
                *prices.data.get(&code).unwrap_or(&1_f64),
                currency.decimal_digits
            )
            .execute(transaction.as_mut())
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
