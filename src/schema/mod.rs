use std::str::FromStr;

use async_graphql::Context;
use ip2country::AsnDB;
use sqlx::SqlitePool;

use crate::{auth::ForwardedHeader, models::currency::Currency};

pub mod mutation;
pub mod query;

pub async fn get_pool_from_context<'ctx>(
    context: &Context<'ctx>,
) -> Result<&'ctx SqlitePool, anyhow::Error> {
    let pool = context
        .data::<SqlitePool>()
        .map_err(|e| anyhow::anyhow!("Cant find pool {:#?}", e))?;
    Ok(pool)
}

pub async fn currency_from_ip(
    pool: &SqlitePool,
    header: &ForwardedHeader,
    db: &AsnDB,
) -> anyhow::Result<Currency> {
    let country_code = header.determine_country(db)?;
    let iso_country = iso_currency::Country::from_str(&country_code)?;
    use strum::IntoEnumIterator;
    for currency in iso_currency::Currency::iter() {
        if currency
            .used_by()
            .iter()
            .any(|country| country == &iso_country)
        {
            let code = currency.code();
            let currency = sqlx::query_as!(Currency, "SELECT * from currency where id=$1", code)
                .fetch_one(pool)
                .await?;
            return Ok(currency);
        }
    }
    Err(anyhow::anyhow!("Currency could not be determined"))
}
