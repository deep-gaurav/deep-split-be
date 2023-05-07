use async_graphql::{Context, Object};

use crate::schema::get_pool_from_context;

use super::{expense::Expense, user::User};

pub struct Split {
    pub id: String,
    pub expense_id: String,
    pub amount: i64,
    pub from_user: String,
    pub to_user: String,
    pub amount_settled: i64,
}

#[Object]
impl Split {
    pub async fn id(&self) -> &str {
        &self.id
    }

    pub async fn expense<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Expense> {
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        Expense::get_from_id(&self.expense_id, pool).await
    }

    pub async fn amount(&self) -> i64 {
        self.amount
    }

    pub async fn amount_settled(&self) -> i64 {
        self.amount_settled
    }

    pub async fn from_user<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        User::get_from_id(&self.from_user, pool).await
    }

    pub async fn to_user<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        User::get_from_id(&self.to_user, pool).await
    }

    pub async fn is_settled(&self) -> bool {
        self.amount_settled == self.amount
    }
}
