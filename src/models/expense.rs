use async_graphql::{Context, Object};
use sqlx::SqlitePool;

use crate::{
    s3::S3,
    schema::{get_pool_from_context, mutation::SplitInput},
};

use super::{
    amount::Amount,
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
    pub currency_id: String,

    pub category: String,

    pub note: Option<String>,
    pub image_id: Option<String>,
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

    pub async fn category(&self) -> &str {
        &self.category
    }

    pub async fn creator_id(&self) -> &str {
        &self.created_by
    }

    pub async fn group<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Group> {
        let pool = get_pool_from_context(context).await?;
        Group::get_from_id(&self.group_id, pool).await
    }

    pub async fn amount(&self) -> Amount {
        Amount {
            amount: self.amount,
            currency_id: self.currency_id.clone(),
        }
    }

    pub async fn splits<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Split>> {
        let pool = get_pool_from_context(context).await?;

        self.get_splits(pool).await
    }

    pub async fn image_id(&self) -> &Option<String> {
        &self.image_id
    }

    pub async fn note(&self) -> &Option<String> {
        &self.note
    }

    pub async fn image_url<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Option<String>> {
        let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        if let Some(id) = &self.image_id {
            Ok(Some(s3.get_public_url(id)))
        } else {
            Ok(None)
        }
    }
}

impl Expense {
    pub async fn new_expense(
        user_id: &str,
        title: &str,
        group_id: &str,
        amount: &Amount,
        splits: Vec<SplitInput>,
        category: &str,
        note: Option<String>,
        image_id: Option<String>,
        s3: &S3,
        pool: &SqlitePool,
    ) -> anyhow::Result<Expense> {
        let mut transaction = pool.begin().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let time = chrono::Utc::now().to_rfc3339();
        let expense = sqlx::query_as!(
            Expense,
            r#"INSERT INTO expenses(id, title, created_at, created_by, group_id, amount, currency_id, category, note, image_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING
            id as "id!", title as "title!", created_at as "created_at!", created_by as "created_by!", group_id as "group_id!", amount as "amount!", currency_id as "currency_id!", category as "category!", note, image_id
            "#,
            id,
            title,
            time,
            user_id,
            group_id,
            amount.amount,
            amount.currency_id,
            category,
            note,
            image_id
        ).fetch_one(transaction.as_mut()).await?;
        let ttype = TransactionType::ExpenseSplit.to_string();

        for split in splits.iter() {
            let id = uuid::Uuid::new_v4().to_string();

            let _data = sqlx::query!("
                INSERT INTO split_transactions(id,expense_id,amount,currency_id,from_user,to_user,transaction_type,created_at,created_by, group_id)
                VALUES ($1, $2, $3,$4,$5,$6,$7,$8, $9,$10)
            ",
            id,
            expense.id,
            split.amount,
            amount.currency_id,
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
        if let Some(image_id) = image_id {
            s3.move_to_be(&image_id).await?;
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
