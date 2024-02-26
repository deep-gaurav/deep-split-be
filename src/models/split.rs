use std::str::FromStr;

use async_graphql::{Context, Enum, Object};
use sqlx::SqlitePool;
use strum::{Display, EnumString};

use crate::schema::get_pool_from_context;

use super::{amount::Amount, expense::Expense, group::Group, user::User};

pub struct Split {
    pub id: String,
    pub expense_id: Option<String>,

    pub group_id: String,
    pub amount: i64,
    pub currency_id: String,
    pub from_user: String,
    pub to_user: String,
    pub transaction_type: String,
    pub part_transaction: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub with_group_id: Option<String>,

    pub note: Option<String>,
    pub image_id: Option<String>,

    pub updated_at: String,
    pub transaction_at: String,

    pub transaction_metadata: Option<String>,
}

#[Object]
impl Split {
    pub async fn id(&self) -> &str {
        &self.id
    }

    pub async fn expense<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Option<Expense>> {
        match &self.expense_id {
            Some(expense_id) => {
                let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
                Expense::get_from_id(expense_id, pool).await.map(Some)
            }
            None => Ok(None),
        }
    }

    pub async fn expense_id(&self) -> Option<String> {
        self.expense_id.clone()
    }

    pub async fn group<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Group> {
        let pool = get_pool_from_context(context).await?;
        Group::get_from_id(&self.group_id, pool).await
    }

    pub async fn group_id(&self) -> &str {
        &self.group_id
    }

    pub async fn amount(&self) -> Amount {
        Amount {
            amount: self.amount,
            currency_id: self.currency_id.clone(),
        }
    }

    pub async fn from_user<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        User::get_from_id(&self.from_user, pool).await
    }

    pub async fn to_user<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        User::get_from_id(&self.to_user, pool).await
    }

    pub async fn transaction_type(&self) -> TransactionType {
        self.get_transaction_type()
    }

    pub async fn created_at(&self) -> &str {
        &self.created_at
    }

    pub async fn creator<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool = get_pool_from_context(context).await?;
        User::get_from_id(&self.created_by, pool).await
    }

    pub async fn creator_id(&self) -> &str {
        &self.created_by
    }

    pub async fn to_user_id(&self) -> &str {
        &self.to_user
    }

    pub async fn from_user_id(&self) -> &str {
        &self.from_user
    }

    pub async fn transaction_part_group_id(&self) -> Option<String> {
        self.part_transaction.clone()
    }

    pub async fn with_group<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Option<Group>> {
        let pool = get_pool_from_context(context).await?;
        if let Some(group_id) = &self.with_group_id {
            Ok(Some(Group::get_from_id(group_id, pool).await?))
        } else {
            Ok(None)
        }
    }

    pub async fn with_group_id(&self) -> Option<String> {
        self.with_group_id.clone()
    }

    pub async fn image_id(&self) -> &Option<String> {
        &self.image_id
    }

    pub async fn note(&self) -> &Option<String> {
        &self.note
    }

    pub async fn siblings<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Split>> {
        let pool = get_pool_from_context(context).await?;
        if let Some(part) = &self.part_transaction {
            let splits = sqlx::query_as!(
                Split,
                "SELECT * FROM split_transactions WHERE part_transaction=$1 AND id!=$2",
                part,
                self.id
            )
            .fetch_all(pool)
            .await?;
            Ok(splits)
        } else {
            Ok(vec![])
        }
    }

    pub async fn updated_at(&self) -> &str {
        &self.updated_at
    }

    pub async fn transaction_at(&self) -> &str {
        &self.transaction_at
    }

    pub async fn transaction_metadata(&self) -> &Option<String> {
        &self.transaction_metadata
    }
}

impl Split {
    pub fn get_transaction_type(&self) -> TransactionType {
        TransactionType::from_str(&self.transaction_type).unwrap_or(TransactionType::CashPaid)
    }

    pub async fn get_from_id(id: &str, pool: &SqlitePool) -> anyhow::Result<Split> {
        let split = sqlx::query_as!(Split, "SELECT * FROM split_transactions WHERE id=$1", id)
            .fetch_one(pool)
            .await?;
        Ok(split)
    }
}

#[derive(EnumString, Enum, Clone, Copy, PartialEq, Eq, Display)]

pub enum TransactionType {
    ExpenseSplit,
    CrossGroupSettlement,
    CurrencyConversion,
    CashPaid,
}
