use std::str::FromStr;

use async_graphql::{Context, CustomValidator, InputValueError, ScalarType};
use ip2country::AsnDB;
use once_cell::sync::Lazy;
use regex::Regex;
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

static UPI_ID_VALIDATOR_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9.\-_]{2,49}@[a-zA-Z._]{2,49}$").unwrap());

pub struct NameValidator {
    field_name: &'static str,
}

impl NameValidator {
    pub fn new(name: &'static str) -> Self {
        Self { field_name: name }
    }
}

impl CustomValidator<String> for NameValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if value.trim().len() > 3 && value.trim().len() < 60 {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Invalid {}",
                self.field_name
            )))
        }
    }
}

pub struct DateTimeValidator {
    field_name: &'static str,
}

impl DateTimeValidator {
    pub fn new(name: &'static str) -> Self {
        Self { field_name: name }
    }
}

impl CustomValidator<String> for DateTimeValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if chrono::DateTime::parse_from_rfc3339(value).is_ok() {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Invalid {}",
                self.field_name
            )))
        }
    }
}

pub struct IdValidator {
    field_name: &'static str,
}

impl IdValidator {
    pub fn new(name: &'static str) -> Self {
        Self { field_name: name }
    }
}

impl CustomValidator<String> for IdValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if uuid::Uuid::parse_str(value).is_ok() {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Invalid {}",
                self.field_name
            )))
        }
    }
}

pub struct UpiIdValidator {
    field_name: &'static str,
}

impl UpiIdValidator {
    pub fn new(name: &'static str) -> Self {
        Self { field_name: name }
    }
}

impl CustomValidator<String> for UpiIdValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if UPI_ID_VALIDATOR_REGEX.is_match(value) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Invalid {}",
                self.field_name
            )))
        }
    }
}
