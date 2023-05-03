use async_graphql::Context;
use sqlx::SqlitePool;

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
