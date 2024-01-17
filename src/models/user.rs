use async_graphql::{Context, Object, SimpleObject};
use sqlx::SqlitePool;

use crate::{auth::AuthTypes, schema::get_pool_from_context};

use super::{amount::Amount, group::Group};

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub name: Option<String>,
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

    pub async fn set_user_name(
        id: &str,
        name: &str,
        default_currency_id: String,
        pool: &SqlitePool,
    ) -> anyhow::Result<User> {
        let mut transaction = pool.begin().await?;
        let user = sqlx::query_as!(
            User,
            r#"UPDATE users SET name = $2 where id=$1 RETURNING id as "id!", name, phone, email, notification_token"#,
            id,
            name
        )
        .fetch_one(transaction.as_mut())
        .await?;

        sqlx::query!(
            "INSERT INTO user_config(user_id,default_currency_id) VALUES ($1, $2)",
            user.id,
            default_currency_id
        )
        .execute(transaction.as_mut())
        .await?;
        Ok(user)
    }

    pub async fn new_invite_user(
        id: &str,
        email: String,
        pool: &SqlitePool,
    ) -> anyhow::Result<User> {
        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users(id,email) VALUES ($1,$2) RETURNING id as "id!", name, phone, email, notification_token"#,
            id,
            email
        )
        .fetch_one(pool)
        .await?;
        Ok(user)
    }

    pub async fn new_user(
        id: &str,
        name: &str,
        phone: Option<String>,
        email: Option<String>,
        upi_id: Option<String>,
        default_currency_id: String,
        pool: &SqlitePool,
    ) -> anyhow::Result<User> {
        let mut transaction = pool.begin().await?;
        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users(id,name,phone,email) VALUES ($1,$2,$3,$4) RETURNING id as "id!", name, phone, email, notification_token"#,
            id,
            name,
            phone,
            email
        )
        .fetch_one(transaction.as_mut())
        .await?;
        sqlx::query!(
            "INSERT INTO user_config(user_id,default_currency_id) VALUES ($1, $2)",
            user.id,
            default_currency_id
        )
        .execute(transaction.as_mut())
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

    pub async fn get_owes_with_group(
        to_user: &str,
        from_user: &str,
        pool: &SqlitePool,
    ) -> anyhow::Result<Vec<OwedInGroup>> {
        let to_pay = sqlx::query!(
            "
            SELECT group_id, currency_id, SUM(net_owed_amount) AS total_net_owed_amount 
            FROM (
                SELECT 
                    from_user,
                    to_user,
                    group_id,
                    currency_id,
                    SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
                FROM 
                    split_transactions
                WHERE 
                    (from_user = $1 AND to_user = $2) OR 
                    (from_user = $2 AND to_user = $1)
                GROUP BY 
                    from_user, to_user, group_id, currency_id
            ) GROUP BY group_id, currency_id
            ",
            from_user,
            to_user,
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|f| OwedInGroup {
            group_id: f.group_id,
            amount: Amount {
                amount: f.total_net_owed_amount,
                currency_id: f.currency_id,
            },
        })
        .collect();
        Ok(to_pay)
    }

    // pub async fn settle_expense(
    //     &self,
    //     to_user: &str,
    //     amount: i64,
    //     pool: &SqlitePool,
    // ) -> anyhow::Result<()> {
    //     let mut transaction = pool.begin().await?;

    //     let amount_remaining =
    //         Self::settle_for_user(&self.id, to_user, &mut transaction, amount).await?;
    //     if amount_remaining > 0 {
    //         return Err(anyhow::anyhow!("Cant settle more than owed"));
    //     }
    //     transaction.commit().await?;

    //     Ok(())
    // }

    // pub async fn settle_for_user<'a>(
    //     from_user: &str,
    //     to_user: &str,
    //     transaction: &mut sqlx::Transaction<'a, sqlx::Sqlite>,
    //     amount: i64,
    // ) -> Result<i64, anyhow::Error> {
    //     let group_owed = sqlx::query!(
    //         r#"

    //         SELECT
    //         from_user,
    //         to_user,
    //         SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
    //     FROM
    //         split_transactions
    //     WHERE
    //         ((from_user = $1) OR
    //         (to_user = $1))
    //     GROUP BY
    //         from_user, to_user, group_id
    //         "#,
    //         &from_user,
    //     )
    //     .fetch_all(transaction.as_mut())
    //     .await?;

    //     let splits = sqlx::query!(
    //         "SELECT * FROM split_transactions WHERE from_user=$1 AND to_user=$2",
    //         from_user,
    //         to_user
    //     )
    //     .fetch_all(transaction.as_mut())
    //     .await?;
    //     let mut amount_remaining = amount;
    //     for split in splits {
    //         if amount_remaining <= 0 {
    //             break;
    //         }
    //         if split.amount - amount_remaining > 0 {
    //             let setlleable = amount_remaining.min(split.amount - split.amount_settled);
    //             let new_val = split.amount_settled + setlleable;
    //             sqlx::query!(
    //                 "UPDATE split_transactions SET amount_settled=$1 WHERE id=$2",
    //                 new_val,
    //                 split.id
    //             )
    //             .execute(transaction.as_mut())
    //             .await?;
    //             amount_remaining -= setlleable;
    //         }
    //     }
    //     Ok(amount_remaining)
    // }
}

#[Object]
impl User {
    pub async fn id(&self) -> &str {
        &self.id
    }

    pub async fn name(&self) -> &Option<String> {
        &self.name
    }

    pub async fn phone(&self) -> &Option<String> {
        &self.phone
    }

    pub async fn email(&self) -> &Option<String> {
        &self.email
    }

    pub async fn is_signed_up(&self) -> bool {
        self.name.is_some()
    }

    pub async fn owes<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<OwedInGroup>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        if self.id == user.id {
            Ok(vec![])
        } else {
            Self::get_owes_with_group(&self.id, &user.id, pool).await
        }
    }

    // pub async fn owed<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
    //     let user = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //         .as_authorized_user()
    //         .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;
    //     if self.id == user.id {
    //         let to_pay = sqlx::query!(
    //             "
    //             SELECT SUM(net_owed_amount) AS total_net_owed_amount
    //             FROM (
    //                 SELECT
    //                     SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
    //                 FROM
    //                     split_transactions
    //                 WHERE
    //                     from_user = $1 OR
    //                     to_user = $1
    //             ) AS subquery_alias;
    //         ",
    //             self.id
    //         )
    //         .fetch_one(pool)
    //         .await?
    //         .total_net_owed_amount;
    //         Ok(to_pay.unwrap_or_default())
    //     } else {
    //         let to_pay = sqlx::query!(
    //             "
    //         SELECT
    //             from_user,
    //             to_user,
    //             SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
    //         FROM
    //             split_transactions
    //         WHERE
    //             (from_user = $1 AND to_user = $2) OR
    //             (from_user = $2 AND to_user = $1)
    //         GROUP BY
    //             from_user, to_user
    //     ",
    //             user.id,
    //             self.id,
    //         )
    //         .fetch_one(pool)
    //         .await?
    //         .net_owed_amount;
    //         Ok(to_pay)
    //     }
    // }

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
}

#[derive(SimpleObject)]
pub struct OwedInGroup {
    pub group_id: String,
    pub amount: Amount,
}

#[derive(SimpleObject)]
pub struct UserConfig {
    pub user_id: String,
    pub default_currency_id: String,
}
