use async_graphql::{Context, Object};
use sqlx::SqlitePool;

use crate::{auth::AuthTypes, schema::get_pool_from_context};

use super::group::Group;

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,

    pub notification_token: Option<String>,
}

impl User {
    pub async fn get_from_phone(phone: &str, pool: &SqlitePool) -> anyhow::Result<User> {
        let user = sqlx::query_as!(User, "SELECT * from users WHERE phone = $1", phone)
            .fetch_one(pool)
            .await?;
        Ok(user)
    }
    pub async fn get_from_email(email: &str, pool: &SqlitePool) -> anyhow::Result<User> {
        let user = sqlx::query_as!(User, "SELECT * from users WHERE email = $1", email)
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

    // pub async

    pub async fn new_user(
        id: &str,
        name: &str,
        phone: Option<String>,
        email: Option<String>,
        upi_id: Option<String>,
        pool: &SqlitePool,
    ) -> anyhow::Result<User> {
        let mut transaction = pool.begin().await?;
        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users(id,name,phone,email) VALUES ($1,$2,$3,$4) RETURNING id as "id!", name as "name!", phone, email, notification_token"#,
            id,
            name,
            phone,
            email
        )
        .fetch_one(transaction.as_mut())
        .await?;
        if let Some(upi_id) = upi_id {
            let id = uuid::Uuid::new_v4().to_string();
            log::debug!("id={id} user_id: {} upi_id: {upi_id}", user.id);
            let upi_id = sqlx::query!(
                "INSERT INTO payment_modes(id,mode,user_id,value) VALUES ($1,$2,$3,$4)",
                id,
                "UPI",
                user.id,
                upi_id
            )
            .execute(transaction.as_mut())
            .await
            .map_err(|e| {
                log::warn!("Error {:#?}", e);
                e
            })?;
            if upi_id.rows_affected() != 1 {
                return Err(anyhow::anyhow!("Cannot add payment method"));
            }
        }
        transaction.commit().await?;
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

    pub async fn settle_expense(
        &self,
        to_user: &str,
        amount: i64,
        pool: &SqlitePool,
    ) -> anyhow::Result<()> {
        let mut transaction = pool.begin().await?;

        let amount_remaining =
            Self::settle_for_user(&self.id, to_user, &mut transaction, amount).await?;
        if amount_remaining > 0 {
            return Err(anyhow::anyhow!("Cant settle more than owed"));
        }
        transaction.commit().await?;

        Ok(())
    }

    pub async fn settle_for_user<'a>(
        from_user: &str,
        to_user: &str,
        transaction: &mut sqlx::Transaction<'a, sqlx::Sqlite>,
        amount: i64,
    ) -> Result<i64, anyhow::Error> {
        let splits = sqlx::query!(
            "SELECT * FROM split_transactions WHERE from_user=$1 AND to_user=$2",
            from_user,
            to_user
        )
        .fetch_all(transaction.as_mut())
        .await?;
        let mut amount_remaining = amount;
        for split in splits {
            if amount_remaining <= 0 {
                break;
            }
            if split.amount - amount_remaining > 0 {
                let setlleable = amount_remaining.min(split.amount - split.amount_settled);
                let new_val = split.amount_settled + setlleable;
                sqlx::query!(
                    "UPDATE split_transactions SET amount_settled=$1 WHERE id=$2",
                    new_val,
                    split.id
                )
                .execute(transaction.as_mut())
                .await?;
                amount_remaining -= setlleable;
            }
        }
        Ok(amount_remaining)
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

    pub async fn phone(&self) -> &Option<String> {
        &self.phone
    }

    pub async fn email(&self) -> &Option<String> {
        &self.email
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

    pub async fn upi_ids<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<String>> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        let id = sqlx::query!(
            r#"SELECT * from payment_modes WHERE user_id = $1 AND mode='UPI'"#,
            self.id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|rec| rec.value)
        .collect();
        Ok(id)
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
