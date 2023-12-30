use async_graphql::{Context, Object};
use sqlx::SqlitePool;

use crate::{auth::AuthTypes, schema::get_pool_from_context};

use super::{expense::Expense, user::User};

pub struct Group {
    pub id: String,
    pub name: Option<String>,
    pub created_at: String,
    pub creator_id: String,
}

#[Object]
impl Group {
    pub async fn id(&self) -> &str {
        &self.id
    }
    pub async fn name(&self) -> &Option<String> {
        &self.name
    }
    pub async fn created_at(&self) -> &str {
        &self.created_at
    }

    pub async fn creator<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<User> {
        let pool = get_pool_from_context(context).await?;
        User::get_from_id(&self.creator_id, pool).await
    }

    pub async fn members<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<User>> {
        let pool = get_pool_from_context(context).await?;
        self.get_users(pool).await
    }

    pub async fn expenses<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(default = 0)] skip: u32,
        #[graphql(default = 10)] limit: u32,
    ) -> anyhow::Result<Vec<Expense>> {
        let pool = get_pool_from_context(context).await?;
        self.get_expenses(skip, limit, pool).await
    }

    pub async fn to_pay<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        let to_pay = sqlx::query!(
            "
            SELECT SUM(split_transactions.amount-split_transactions.amount_settled) as to_pay FROM 
            split_transactions
            JOIN expenses ON expenses.id=split_transactions.expense_id
            JOIN groups ON groups.id = expenses.group_id

            WHERE groups.id = $1 AND split_transactions.from_user = $2
        ",
            self.id,
            user.id
        )
        .fetch_one(pool)
        .await?
        .to_pay
        .unwrap_or_default();
        Ok(to_pay)
    }

    pub async fn to_receive<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        let to_pay = sqlx::query!(
            "
            SELECT SUM(split_transactions.amount-split_transactions.amount_settled) as to_pay FROM 
            split_transactions
            JOIN expenses ON expenses.id=split_transactions.expense_id
            JOIN groups ON groups.id = expenses.group_id

            WHERE groups.id = $1 AND split_transactions.to_user = $2
        ",
            self.id,
            user.id
        )
        .fetch_one(pool)
        .await?
        .to_pay
        .unwrap_or_default();
        Ok(to_pay)
    }
}

impl Group {
    pub async fn get_from_id(id: &str, pool: &SqlitePool) -> anyhow::Result<Group> {
        let group = sqlx::query_as!(Group, "SELECT * from groups WHERE id = $1", id)
            .fetch_one(pool)
            .await?;
        Ok(group)
    }

    pub async fn create_group(
        id: &str,
        creator_id: &str,
        name: Option<String>,
        pool: &SqlitePool,
    ) -> anyhow::Result<Group> {
        let mut transaction = pool.begin().await?;

        let current_time = chrono::Utc::now().to_rfc3339();
        let group = sqlx::query_as!(
            Group,
        r#"INSERT INTO groups(id,name,created_at,creator_id) VALUES ($1,$2,$3,$4) RETURNING id as "id!", name, created_at as "created_at!", creator_id as "creator_id!""#,
        id,
        name,
        current_time,
        creator_id
    ).fetch_one
        (transaction.as_mut())
        .await?;

        let membership_id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO group_memberships(id,user_id,group_id) VALUES ($1,$2,$3)",
            membership_id,
            creator_id,
            group.id
        )
        .execute(transaction.as_mut())
        .await?;
        transaction.commit().await?;
        Ok(group)
    }

    pub async fn add_to_group(
        group_id: &str,
        user_id: &str,
        pool: &SqlitePool,
    ) -> anyhow::Result<()> {
        let membership_id = uuid::Uuid::new_v4().to_string();

        let _group_membership = sqlx::query!(
            "INSERT INTO group_memberships(id,user_id,group_id) VALUES ($1,$2,$3)",
            membership_id,
            user_id,
            group_id,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_users(&self, pool: &SqlitePool) -> anyhow::Result<Vec<User>> {
        let users = sqlx::query_as!(
            User,
            r#"
            SELECT users.* FROM 
                users JOIN group_memberships ON users.id=group_memberships.user_id 
                JOIN groups ON group_memberships.group_id=groups.id AND groups.id=$1
            "#,
            self.id
        )
        .fetch_all(pool)
        .await?;
        Ok(users)
    }

    pub async fn get_expenses(
        &self,
        skip: u32,
        limit: u32,
        pool: &SqlitePool,
    ) -> anyhow::Result<Vec<Expense>> {
        let expenses = sqlx::query_as!(
            Expense,
            r#"SELECT 
            id as "id!", title as  "title!", amount as "amount!", created_at as "created_at!", group_id as "group_id!", created_by as "created_by!" 
            FROM expenses where group_id=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"#,
            self.id,
            limit,
            skip
        )
        .fetch_all(pool)
        .await?;
        Ok(expenses)
    }
}
