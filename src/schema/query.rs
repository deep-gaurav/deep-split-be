use async_graphql::{Context, Object, SimpleObject, Union};
use sqlx::SqlitePool;

use crate::{
    auth::AuthTypes,
    models::{expense::Expense, group::Group, split::Split, user::User},
};

use super::get_pool_from_context;

pub struct Query;

#[Object]
impl Query {
    pub async fn ping(&self) -> String {
        "pong".into()
    }

    pub async fn user<'a>(&self, context: &Context<'a>) -> anyhow::Result<UserAuth<'a>> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(claims) => Ok(UserAuth::Unregistered(Unregistered {
                phone: claims.phone_number.clone(),
                email: claims.email.clone(),
            })),
            AuthTypes::AuthorizedUser(user) => Ok(UserAuth::Registered(Registered { user })),
        }
    }

    pub async fn group<'a>(&self, context: &Context<'a>, id: String) -> anyhow::Result<Group> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_claims) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(user) => {
                let pool = get_pool_from_context(context).await?;
                let group = Group::get_from_id(&id, pool)
                    .await
                    .map_err(|_e| anyhow::anyhow!("Group not found"))?;
                let users = group.get_users(pool).await?;
                if users.iter().any(|u| u.id == user.id) {
                    Ok(group)
                } else {
                    Err(anyhow::anyhow!("Unauthorized"))
                }
            }
        }
    }

    pub async fn user_by_id<'a>(&self, context: &Context<'a>, id: String) -> anyhow::Result<User> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        let _ = auth_type
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;

        let user = User::get_from_id(&id, pool).await?;

        Ok(user)
    }

    // pub async fn expenses_with_user<'ctx>(
    //     &self,
    //     context: &Context<'ctx>,
    //     user_id: String,
    //     #[graphql(default = 0)] skip: u32,
    //     #[graphql(default = 10)] limit: u32,
    // ) -> anyhow::Result<Vec<Expense>> {
    //     let auth_type = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
    //     let user = auth_type
    //         .as_authorized_user()
    //         .ok_or(anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;
    //     Self::get_expenses_with_user(&user.id, &user_id, skip, limit, pool).await
    // }

    pub async fn interacted_users<'a>(&self, context: &Context<'a>) -> anyhow::Result<Vec<User>> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;

        let users = sqlx::query_as!(
            User,
            r#"
            SELECT DISTINCT users.* FROM users 
                JOIN group_memberships ON group_memberships.user_id = users.id
                
            WHERE group_memberships.group_id IN (SELECT groups.id FROM 
                users JOIN group_memberships ON users.id=group_memberships.user_id AND users.id=$1
                JOIN groups ON group_memberships.group_id=groups.id)
        "#,
            _user.id
        )
        .fetch_all(pool)
        .await?;
        Ok(users)
    }

    pub async fn groups<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Group>> {
        let auth = context
            .data::<AuthTypes>()
            .map_err(|_e| anyhow::anyhow!("Unauthorized"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;

        let pool = get_pool_from_context(context).await?;
        let groups = auth.get_groups(pool).await?;
        Ok(groups)
    }

    pub async fn find_user_by_email<'ctx>(
        &self,
        context: &Context<'ctx>,
        email: String,
    ) -> anyhow::Result<User> {
        let _auth = context
            .data::<AuthTypes>()
            .map_err(|_e| anyhow::anyhow!("Unauthorized"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        let user = User::get_from_email(&email, pool).await;

        if let Ok(user) = user {
            Ok(user)
        } else {
            Err(anyhow::anyhow!("No User with given email"))
        }
    }

    pub async fn overall_owed<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<i64> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let to_pay = sqlx::query!(
            "
            SELECT SUM(net_owed_amount) as total_net_owed_amount FROM (
                SELECT 
                    from_user,
                    to_user,
                    SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
                FROM 
                    split_transactions
                WHERE 
                    (from_user = $1) OR 
                    (to_user = $1)
                GROUP BY 
                    from_user, to_user
            )
        ",
            user.id
        )
        .fetch_one(pool)
        .await?
        .total_net_owed_amount;
        Ok(to_pay.unwrap_or_default())
    }

    pub async fn get_transactions_with_user<'ctx>(
        &self,
        context: &Context<'ctx>,
        with_user: String,
        from_time: Option<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<Split>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let splits = if let Some(from_tile) = from_time {
            sqlx::query_as!(
                Split,
                "
                SELECT * from split_transactions WHERE
                ((from_user = $1 AND to_user = $2) OR (from_user = $2 AND to_user = $1))
                AND created_at < $4
                ORDER BY created_at DESC LIMIT $3
                ",
                user.id,
                with_user,
                limit,
                from_tile
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Split,
                "
                SELECT * from split_transactions WHERE
                (from_user = $1 AND to_user = $2) OR (from_user = $2 AND to_user = $1)
                ORDER BY created_at DESC LIMIT $3
                ",
                user.id,
                with_user,
                limit,
            )
            .fetch_all(pool)
            .await?
        };
        Ok(splits)
    }

    pub async fn get_transactions_with_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        with_group: String,
        from_time: Option<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<Split>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let splits = if let Some(from_tile) = from_time {
            sqlx::query_as!(
                Split,
                "
                SELECT * from split_transactions WHERE
                (from_user = $1  OR to_user = $1)
                AND group_id = $2
                AND created_at < $4
                ORDER BY created_at DESC LIMIT $3
                ",
                user.id,
                with_group,
                limit,
                from_tile
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Split,
                "
                SELECT * from split_transactions WHERE
                (from_user = $1  OR to_user = $1)
                AND group_id = $2
                ORDER BY created_at DESC LIMIT $3
                ",
                user.id,
                with_group,
                limit,
            )
            .fetch_all(pool)
            .await?
        };
        Ok(splits)
    }
}

impl Query {
    // pub async fn get_expenses_by_creator(
    //     &self,
    //     user_id: &str,
    //     skip: u32,
    //     limit: u32,
    //     pool: &SqlitePool,
    // ) -> anyhow::Result<Vec<Expense>> {
    //     let expenses = sqlx::query_as!(
    //         Expense,
    //         r#"SELECT
    //         id as "id!", title as  "title!", amount as "amount!", created_at as "created_at!", group_id as "group_id!", created_by as "created_by!"
    //         FROM expenses where created_by=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"#,
    //         user_id,
    //         limit,
    //         skip
    //     )
    //     .fetch_all(pool)
    //     .await?;
    //     Ok(expenses)
    // }

    pub async fn get_expenses_with_user(
        user_1: &str,
        user_2: &str,
        from_time: Option<String>,
        limit: u32,
        pool: &SqlitePool,
    ) -> anyhow::Result<Vec<Expense>> {
        let expenses = if let Some(from_time) = from_time {
            sqlx::query_as!(
                Expense,
                r#"
    SELECT e.id, e.title, e.created_at as created_at, e.created_by, e.group_id, e.amount
    FROM expenses e
    JOIN split_transactions s ON e.id = s.expense_id
    WHERE ((s.from_user = $1 AND s.to_user = $2)
    OR (s.from_user = $2 AND s.to_user = $1))
    AND s.created_at < $4
    ORDER BY created_at DESC LIMIT $3
                "#,
                user_1,
                user_2,
                limit,
                from_time
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Expense,
                r#"
    SELECT e.id, e.title, e.created_at as created_at, e.created_by, e.group_id, e.amount
    FROM expenses e
    JOIN split_transactions s ON e.id = s.expense_id
    WHERE (s.from_user = $1 AND s.to_user = $2)
    OR (s.from_user = $2 AND s.to_user = $1)
    ORDER BY created_at DESC LIMIT $3
                "#,
                user_1,
                user_2,
                limit,
            )
            .fetch_all(pool)
            .await?
        };
        Ok(expenses)
    }
}

#[derive(Union)]
pub enum UserAuth<'a> {
    Unregistered(Unregistered),
    Registered(Registered<'a>),
}

#[derive(SimpleObject)]
pub struct Unregistered {
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(SimpleObject)]
pub struct Registered<'a> {
    pub user: &'a User,
}
