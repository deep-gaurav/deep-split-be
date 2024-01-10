use async_graphql::SimpleObject;
use sqlx::SqlitePool;

#[derive(SimpleObject)]
pub struct Currency {
    pub id: String,
    pub display_name: String,
    pub rate: f64,
    pub symbol: String,
}

impl Currency {
    pub async fn get_all(pool: &SqlitePool) -> anyhow::Result<Vec<Self>> {
        let currencies = sqlx::query_as!(Currency, "SELECT * FROM currency")
            .fetch_all(pool)
            .await?;
        Ok(currencies)
    }
}
