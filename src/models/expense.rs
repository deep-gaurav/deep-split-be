use async_graphql::{Context, Object};
use sqlx::SqlitePool;

use crate::schema::{get_pool_from_context, mutation::SplitInput};

use super::{
    group::Group,
    split::{Split, TransactionType},
    user::User,
};

pub struct Expense {
    pub id: String,
    pub title: String,

    pub created_at: String,
    pub created_by: String,

    pub group_id: String,

    pub amount: i64,
}

#[Object]
impl Expense {
    pub async fn id(&self) -> &str {
        &self.id
    }

    pub async fn title(&self) -> &str {
        &self.title
    }

    pub async fn created_at(&self) -> &str {
        &self.created_at
    }

    pub async fn creator<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool = get_pool_from_context(context).await?;
        User::get_from_id(&self.created_by, pool).await
    }

    pub async fn group<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Group> {
        let pool = get_pool_from_context(context).await?;
        Group::get_from_id(&self.group_id, pool).await
    }

    pub async fn amount(&self) -> i64 {
        self.amount
    }

    pub async fn splits<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Split>> {
        let pool = get_pool_from_context(context).await?;

        self.get_splits(pool).await
    }
}

impl Expense {
    pub async fn new_expense(
        user_id: &str,
        title: &str,
        group_id: &str,
        amount: i64,
        splits: Vec<SplitInput>,
        pool: &SqlitePool,
    ) -> anyhow::Result<Expense> {
        let mut transaction = pool.begin().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let time = chrono::Utc::now().to_rfc3339();
        let expense = sqlx::query_as!(
            Expense,
            r#"INSERT INTO expenses(id, title, created_at, created_by, group_id, amount)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
            id as "id!", title as "title!", created_at as "created_at!", created_by as "created_by!", group_id as "group_id!", amount as "amount!"
            "#,
            id,
            title,
            time,
            user_id,
            group_id,
            amount,
        ).fetch_one(transaction.as_mut()).await?;
        let ttype = TransactionType::ExpenseSplit.to_string();

        for split in splits.iter() {
            let id = uuid::Uuid::new_v4().to_string();

            let _data = sqlx::query!("
                INSERT INTO split_transactions(id,expense_id,amount,from_user,to_user,transaction_type,created_at,created_by, group_id)
                VALUES ($1, $2, $3,$4,$5,$6,$7,$8, $9)
            ",
            id,
            expense.id,
            split.amount,
            split.user_id,
            user_id,
            ttype,
            time,
            user_id,
            group_id,
        ).execute(transaction.as_mut()).await.map_err(|e|
       { log::warn!("FAILED {e:#?} VALUES id:{} expense:{} split_amount:{} userid:{} split_user:{}, amount:{}",
       id,
       expense.id,
       split.amount,
       user_id,
       split.user_id,
       0
    );
            e}
        )?;
        }
        transaction.commit().await?;
        Ok(expense)
    }

    pub async fn get_from_id(id: &str, pool: &SqlitePool) -> anyhow::Result<Expense> {
        let expense = sqlx::query_as!(Expense, "SELECT * FROM expenses WHERE id=$1", id)
            .fetch_one(pool)
            .await?;
        Ok(expense)
    }

    pub async fn get_splits(&self, pool: &SqlitePool) -> anyhow::Result<Vec<Split>> {
        let splits = sqlx::query_as!(
            Split,
            "SELECT * FROM split_transactions WHERE expense_id=$1",
            self.id
        )
        .fetch_all(pool)
        .await?;
        Ok(splits)
    }
}
