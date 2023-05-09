use async_graphql::{Context, Object};
use sqlx::SqlitePool;

use crate::{auth::AuthTypes, schema::get_pool_from_context};

use super::group::Group;

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub phone: String,

    pub notification_token: Option<String>,
}

impl User {
    pub async fn get_from_phone(phone: &str, pool: &SqlitePool) -> anyhow::Result<User> {
        let user = sqlx::query_as!(User, "SELECT * from users WHERE phone = $1", phone)
            .fetch_one(pool)
            .await?;
        Ok(user)
    }

    pub async fn get_from_id(id: &str, pool: &SqlitePool) -> anyhow::Result<User> {
        let user = sqlx::query_as!(User, "SELECT * from users WHERE id = $1", id)
            .fetch_one(pool)
            .await?;
        Ok(user)
    }

    pub async fn new_user(
        id: &str,
        name: &str,
        phone: &str,
        pool: &SqlitePool,
    ) -> anyhow::Result<User> {
        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users(id,name,phone) VALUES ($1,$2,$3) RETURNING id as "id!", name as "name!", phone as "phone!", notification_token"#,
            id,
            name,
            phone
        )
        .fetch_one(pool)
        .await?;
        Ok(user)
    }

    pub async fn get_groups(&self, pool: &SqlitePool) -> anyhow::Result<Vec<Group>> {
        let groups = sqlx::query_as!(
            Group,
            r#"
            SELECT groups.* FROM 
                users JOIN group_memberships ON users.id=group_memberships.user_id AND users.id=$1
                JOIN groups ON group_memberships.group_id=groups.id
            "#,
            self.id
        )
        .fetch_all(pool)
        .await?;
        Ok(groups)
    }
}

#[Object]
impl User {
    pub async fn id(&self) -> &str {
        &self.id
    }

    pub async fn name(&self) -> &str {
        &self.name
    }

    pub async fn phone(&self) -> &str {
        &self.phone
    }

    pub async fn groups<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Group>> {
        let auth = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("Not logged in"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Not logged in"))?;
        if auth.id != self.id {
            return Err(anyhow::anyhow!("Unauthorized"));
        }

        let pool = get_pool_from_context(context).await?;
        let groups = self.get_groups(pool).await?;
        Ok(groups)
    }
    pub async fn to_pay<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        if self.id == user.id {
            let to_pay = sqlx::query!(
                "
                SELECT SUM(amount-amount_settled) as to_pay FROM split_transactions
                WHERE from_user = $1
            ",
                self.id
            )
            .fetch_one(pool)
            .await?
            .to_pay
            .unwrap_or_default();
            Ok(to_pay)
        } else {
            let to_pay = sqlx::query!(
                "
            SELECT SUM(amount-amount_settled) as to_pay FROM split_transactions
            WHERE to_user = $1 AND from_user = $2
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

    pub async fn to_receive<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        if self.id == user.id {
            let to_pay = sqlx::query!(
                "
                SELECT SUM(amount-amount_settled) as to_pay FROM split_transactions
                WHERE to_user = $1
            ",
                self.id,
            )
            .fetch_one(pool)
            .await?
            .to_pay
            .unwrap_or_default();
            Ok(to_pay)
        } else {
            let to_pay = sqlx::query!(
                "
                SELECT SUM(amount-amount_settled) as to_pay FROM split_transactions
                WHERE to_user = $1 AND from_user = $2
            ",
                user.id,
                self.id,
            )
            .fetch_one(pool)
            .await?
            .to_pay
            .unwrap_or_default();
            Ok(to_pay)
        }
    }
}
