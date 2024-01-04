use std::str::FromStr;

use async_graphql::{Context, Enum, Object};
use strum::{Display, EnumString};

use crate::schema::get_pool_from_context;

use super::{expense::Expense, group::Group, user::User};

pub struct Split {
    pub id: String,
    pub expense_id: Option<String>,

    pub group_id: String,
    pub amount: i64,
    pub from_user: String,
    pub to_user: String,
    pub transaction_type: String,
    pub part_transaction: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub with_group_id: Option<String>,
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

    pub async fn group<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Group> {
        let pool = get_pool_from_context(context).await?;
        Group::get_from_id(&self.group_id, pool).await
    }

    pub async fn amount(&self) -> i64 {
        self.amount
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
}

impl Split {
    pub fn get_transaction_type(&self) -> TransactionType {
        TransactionType::from_str(&self.transaction_type).unwrap_or(TransactionType::CashPaid)
    }
}

#[derive(EnumString, Enum, Clone, Copy, PartialEq, Eq, Display)]

pub enum TransactionType {
    ExpenseSplit,
    CrossGroupSettlement,
    CashPaid,
}
