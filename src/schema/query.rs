use std::collections::HashMap;

use async_graphql::{Context, Object, SimpleObject, Union};

use sqlx::SqlitePool;

use crate::{
    auth::AuthTypes,
    models::{
        amount::Amount,
        currency::Currency,
        expense::Expense,
        group::Group,
        split::Split,
        user::{User, UserConfig},
    },
    s3::S3,
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
                let users = Group::get_users(&group.id, pool).await?;
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

    pub async fn expense_by_id<'ctx>(&self, context: &Context<'ctx>, id: String) -> anyhow::Result<Expense>{
        let _user = context
        .data::<AuthTypes>()
        .map_err(|e| anyhow::anyhow!("{e:#?}"))?
        .as_authorized_user()
        .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        
        let expense = Expense::get_from_id(&id, pool).await?;
        Ok(expense)
    }

    pub async fn split_by_id<'ctx>(&self, context: &Context<'ctx>, id:String) -> anyhow::Result<Split>{
        let _user = context
        .data::<AuthTypes>()
        .map_err(|e| anyhow::anyhow!("{e:#?}"))?
        .as_authorized_user()
        .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        
        let split = Split::get_from_id(&id, pool).await?;
        Ok(split)
    }

    pub async fn splits_by_part<'ctx>(&self, context: &Context<'ctx>, part_id:String) -> anyhow::Result<Vec<Split>>{
        let _user = context
        .data::<AuthTypes>()
        .map_err(|e| anyhow::anyhow!("{e:#?}"))?
        .as_authorized_user()
        .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool: &sqlx::Pool<sqlx::Sqlite> = get_pool_from_context(context).await?;
        let split = sqlx::query_as!(Split, "SELECT * FROM split_transactions WHERE part_transaction = $1", part_id).fetch_all(pool).await?;
        Ok(split)
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

    pub async fn server_commit_id(&self) -> &str {
        env!("GIT_HASH")
    }

    pub async fn overall_owed<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Amount>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let to_pay = sqlx::query_as!(
            Amount,
            "
            SELECT currency_id,SUM(net_owed_amount) as amount FROM (
                SELECT 
                    from_user,
                    to_user,
                    currency_id,
                    SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
                FROM 
                    split_transactions
                WHERE 
                    (from_user = $1) OR 
                    (to_user = $1)
                GROUP BY 
                    from_user, to_user, currency_id
            ) GROUP BY currency_id
        ",
            user.id
        )
        .fetch_all(pool)
        .await?;
        Ok(to_pay)
    }

    pub async fn get_transactions_mix_expense_with_user<'ctx>(
        &self,
        context: &Context<'ctx>,
        with_user: String,
        from_time: Option<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<ExpenseMixSplit>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let direct_group =
        Group::find_group_for_users(
            if with_user!=user.id {
                vec![with_user.to_string(), user.id.to_string()]
            }
            else {
                vec![with_user.to_string()]
            },
            pool
        ).await.ok().and_then(|group|
        if group.name.is_none(){
            Some(group.id)
        }else{
            None
        }
        );
        let splits = 
            sqlx::query!(
                r#"
                WITH expenses_left_join AS (
                    SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id
                    FROM expenses e
                    LEFT JOIN split_transactions st ON st.expense_id = e.id AND (st.to_user = $1 OR st.from_user = $1)
                    WHERE e.group_id = $3
                    ),
                    split_transactions_right_join AS (
                        SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id
                    FROM split_transactions st
                    LEFT JOIN expenses e ON st.expense_id = e.id
                    WHERE ((st.to_user = $1 AND st.from_user = $2) OR (st.from_user = $1 AND st.to_user = $2)))
                    
                    SELECT *
                    FROM (
                      SELECT *
                      FROM expenses_left_join
                      UNION ALL
                      SELECT *
                      FROM split_transactions_right_join
                      WHERE expense_id IS NULL
                    )
                    WHERE (COALESCE(expense_created_at, split_transaction_created_at) < $5 OR $5 IS NULL)
                    ORDER BY COALESCE(expense_created_at, split_transaction_created_at) DESC
                    LIMIT $4            
                "#,
                user.id,
                with_user,
                direct_group,
                limit,
                from_time
            )
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row|{
                    let expense = if let Some(expense_id)=row.expense_id.clone(){
                        Some(Expense{
                            id: expense_id,
                            currency_id: row.expense_currency_id.unwrap(),
                            title: row.expense_title.unwrap(),
                            created_at: row.expense_created_at.unwrap(),
                            created_by: row.expense_created_by.unwrap(),
                            group_id: row.expense_group_id.unwrap(),
                            amount: row.expense_amount.unwrap(),
                            category: row.expense_category.unwrap(),
                            note: row.expense_note,
                            image_id: row.expense_image_id,
                        })
                    }else{
                        None
                    };
                    let split = if let Some(split_id)=row.split_transaction_id{
                        Some(Split{
                            id:split_id,
                            currency_id: row.split_transaction_currency_id.unwrap(),
                            amount:row.split_transaction_amount.unwrap(),
                            expense_id:row.expense_id,
                            group_id:row.split_transaction_group_id.unwrap(),
                            from_user: row.split_transaction_from_user.unwrap(),
                            to_user: row.split_transaction_to_user.unwrap(),
                            part_transaction: row.split_transaction_part_transaction,
                            transaction_type:row.split_transaction_transaction_type.unwrap(),
                            created_at:row.split_transaction_created_at.unwrap(),
                            created_by: row.split_transaction_created_by.unwrap(),
                            with_group_id: row.split_transaction_with_group_id,
                            note: row.split_transaction_note,
                            image_id: row.split_transaction_image_id,
                        })
                    }else{
                        None
                    };
                    ExpenseMixSplit { expense, split}
                }
            ).collect();

        Ok(splits)
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

    pub async fn currencies<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<Currency>> {
        let pool = get_pool_from_context(context).await?;
        Currency::get_all(pool).await
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

    pub async fn get_transactions<'ctx>(
        &self,
        context: &Context<'ctx>,
        from_time: Option<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<ExpenseMixSplit>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        let rows = sqlx::query!(
            r#"
            SELECT 
                st.id AS transaction_id,
                st.amount AS transaction_amount,
                st.currency_id AS transaction_currency,
                st.from_user,
                st.to_user,
                st.transaction_type,
                st.part_transaction,
                st.created_at AS transaction_created_at,
                st.created_by AS transaction_created_by,
                st.group_id AS transaction_group_id,
                st.with_group_id,
                st.note AS transaction_note,
                st.image_id AS transaction_image,
                e.id AS expense_id,
                e.title AS expense_title,
                e.created_at AS expense_created_at,
                e.created_by AS expense_created_by,
                e.group_id AS expense_group_id,
                e.currency_id AS expense_currency_id,
                e.amount AS expense_amount,
                e.category as expense_category,
                e.note AS expense_note,
                e.image_id AS expense_image_id
            FROM 
                split_transactions st
            LEFT JOIN 
                expenses e ON st.expense_id = e.id
            WHERE 
                (st.from_user = $1 OR st.to_user = $1)
                AND (st.created_at <= $2 OR $2 IS NULL)
            ORDER BY 
                st.created_at DESC
            LIMIT 
                $3;
            "#,
            user.id,
            from_time,
            limit
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| ExpenseMixSplit {
            expense: if row.expense_id.is_some() {
                Some(Expense {
                    id: row.expense_id.clone().unwrap(),
                    title: row.expense_title.unwrap(),
                    created_at: row.expense_created_at.unwrap(),
                    created_by: row.expense_created_by.unwrap(),
                    group_id: row.expense_group_id.unwrap(),
                    amount: row.expense_amount.unwrap(),
                    currency_id: row.expense_currency_id.unwrap(),
                    category: row.expense_category.unwrap(),
                    note: row.expense_note,
                    image_id: row.expense_image_id,
                })
            } else {
                None
            },
            split: Some(Split {
                id: row.transaction_id,
                expense_id: row.expense_id,
                group_id: row.transaction_group_id,
                amount: row.transaction_amount,
                currency_id: row.transaction_currency,
                from_user: row.from_user,
                to_user: row.to_user,
                transaction_type: row.transaction_type,
                part_transaction: row.part_transaction,
                created_at: row.transaction_created_at,
                created_by: row.transaction_created_by,
                with_group_id: row.with_group_id,
                note: row.transaction_note,
                image_id: row.transaction_image,
            }),
        })
        .collect();

        Ok(rows)
    }

    pub async fn get_transactions_mix_expense_with_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        with_group: String,
        from_time: Option<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<ExpenseMixSplit>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let splits = if let Some(from_tile) = from_time {
            sqlx::query!(
                r#"
                WITH expenses_left_join AS (
                    SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id
                    FROM expenses e
                    LEFT JOIN split_transactions st ON st.expense_id = e.id AND (st.to_user = $1 OR st.from_user = $1)
                    WHERE e.group_id = $2
                    ),
                    split_transactions_right_join AS (
                        SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id
                    FROM split_transactions st
                    LEFT JOIN expenses e ON st.expense_id = e.id
                    WHERE st.to_user = $1 OR st.from_user = $1
                        AND st.group_id = $2
                    )
                    
                    SELECT *
                    FROM (
                      SELECT *
                      FROM expenses_left_join
                      UNION ALL
                      SELECT *
                      FROM split_transactions_right_join
                      WHERE expense_id IS NULL
                    )
                    WHERE COALESCE(expense_created_at, split_transaction_created_at) < $4  
                    ORDER BY COALESCE(expense_created_at, split_transaction_created_at) DESC
                    LIMIT $3            
                "#,
                user.id,
                with_group,
                limit,
                from_tile
            )
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row|{
                    let expense = if let Some(expense_id)=row.expense_id.clone(){
                        Some(Expense{
                            id: expense_id,
                            currency_id: row.expense_currency_id.unwrap(),
                            title: row.expense_title.unwrap(),
                            created_at: row.expense_created_at.unwrap(),
                            created_by: row.expense_created_by.unwrap(),
                            group_id: row.expense_group_id.unwrap(),
                            amount: row.expense_amount.unwrap(),
                            category: row.expense_category.unwrap(),
                            note: row.expense_note,
                            image_id: row.expense_image_id,
                        })
                    }else{
                        None
                    };
                    let split = if let Some(split_id)=row.split_transaction_id{
                        Some(Split{
                            id:split_id,
                            currency_id: row.split_transaction_currency_id.unwrap(),
                            amount:row.split_transaction_amount.unwrap(),
                            expense_id:row.expense_id,
                            group_id:row.split_transaction_group_id.unwrap(),
                            from_user: row.split_transaction_from_user.unwrap(),
                            to_user: row.split_transaction_to_user.unwrap(),
                            part_transaction: row.split_transaction_part_transaction,
                            transaction_type:row.split_transaction_transaction_type.unwrap(),
                            created_at:row.split_transaction_created_at.unwrap(),
                            created_by: row.split_transaction_created_by.unwrap(),
                            with_group_id: row.split_transaction_with_group_id,
                            note: row.split_transaction_note,
                            image_id: row.split_transaction_image_id,
                        })
                    }else{
                        None
                    };
                    ExpenseMixSplit { expense, split}
                }
            ).collect()
        } else {
            sqlx::query!(
                r#"
                WITH expenses_left_join AS (
                    SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id
                    FROM expenses e
                    LEFT JOIN split_transactions st ON st.expense_id = e.id AND (st.to_user = $1 OR st.from_user = $1)
                    WHERE e.group_id = $2
                    ),
                    split_transactions_right_join AS (
                    SELECT
                        st.id AS split_transaction_id,
                        st.amount AS split_transaction_amount,
                        st.from_user as split_transaction_from_user,
                        st.to_user as split_transaction_to_user,
                        st.transaction_type as split_transaction_transaction_type,
                        st.part_transaction as split_transaction_part_transaction,
                        st.created_at AS split_transaction_created_at,
                        st.created_by AS split_transaction_created_by,
                        st.group_id AS split_transaction_group_id,
                        st.with_group_id AS split_transaction_with_group_id,
                        st.currency_id AS split_transaction_currency_id,
                        st.note AS split_transaction_note,
                        st.image_id AS split_transaction_image_id,
                        e.id AS expense_id,
                        e.title as expense_title,
                        e.created_at as expense_created_at,
                        e.created_by as expense_created_by,
                        e.group_id as expense_group_id,
                        e.amount as expense_amount,
                        e.currency_id as expense_currency_id,
                        e.category as expense_category,
                        e.note AS expense_note,
                        e.image_id AS expense_image_id

                    FROM split_transactions st
                    LEFT JOIN expenses e ON st.expense_id = e.id
                    WHERE (st.to_user = $1 OR st.from_user = $1)
                        AND st.group_id = $2
                    )
                    
                    SELECT *
                    FROM (
                      SELECT *
                      FROM expenses_left_join
                      UNION ALL
                      SELECT *
                      FROM split_transactions_right_join
                      WHERE expense_id IS NULL
                    )
                    ORDER BY COALESCE(expense_created_at, split_transaction_created_at) DESC
                    LIMIT $3            
                "#,
                user.id,
                with_group,
                limit,
            )
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row|{
                    let expense = if let Some(expense_id)=row.expense_id.clone(){
                        Some(Expense{
                            id: expense_id,
                            currency_id: row.expense_currency_id.unwrap(),
                            title: row.expense_title.unwrap(),
                            created_at: row.expense_created_at.unwrap(),
                            created_by: row.expense_created_by.unwrap(),
                            group_id: row.expense_group_id.unwrap(),
                            amount: row.expense_amount.unwrap(),
                            category: row.expense_category.unwrap(),
                            note: row.expense_note,
                            image_id: row.expense_image_id,
                        })
                    }else{
                        None
                    };
                    let split = if let Some(split_id)=row.split_transaction_id{
                        Some(Split{
                            id:split_id,
                            amount:row.split_transaction_amount.unwrap(),
                            currency_id: row.split_transaction_currency_id.unwrap(),
                            expense_id:row.expense_id,
                            group_id:row.split_transaction_group_id.unwrap(),
                            from_user: row.split_transaction_from_user.unwrap(),
                            to_user: row.split_transaction_to_user.unwrap(),
                            part_transaction: row.split_transaction_part_transaction,
                            transaction_type:row.split_transaction_transaction_type.unwrap(),
                            created_at:row.split_transaction_created_at.unwrap(),
                            created_by: row.split_transaction_created_by.unwrap(),
                            with_group_id: row.split_transaction_with_group_id,
                            image_id: row.split_transaction_image_id,
                            note: row.split_transaction_note,
                        })
                    }else{
                        None
                    };
                    ExpenseMixSplit { expense, split}

                }
            ).collect()
        };

        Ok(splits)
    }

    pub async fn config<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<UserConfig> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let config = sqlx::query_as!(
            UserConfig,
            "SELECT * From user_config where user_id = $1",
            user.id
        )
        .fetch_one(pool)
        .await?;
        Ok(config)
    }

    pub async fn image_url<'ctx>(
        &self,
        context: &Context<'ctx>,
        id: String,
    ) -> anyhow::Result<String> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(s3.get_public_url(&id))
    }

    pub async fn expense_summary_by_category<'ctx>(&self, context: &Context<'ctx>, group_id: Option<String>, from_time: String) -> anyhow::Result<Vec<CategorisedAmount>>{
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let data = sqlx::query!(r"
        SELECT u.id, e.category,
        CASE WHEN e.created_by=u.id THEN e.amount ELSE 0 END +SUM(CASE WHEN st.to_user = u.id THEN -st.amount
             ELSE COALESCE(st.amount,0)
           END) AS total_spent, 
           e.currency_id
       FROM users AS u
       LEFT JOIN expenses AS e ON e.created_by = u.id AND (e.group_id = $2 OR $2 IS NULL) AND e.created_at > $3
       OR e.id IN (SELECT expense_id FROM split_transactions WHERE from_user = u.id AND (group_id = $2 OR $2 IS NULL) AND created_at > $3)
       LEFT JOIN split_transactions AS st ON st.expense_id = e.id AND (st.group_id = $2 OR $2 IS NULL) AND (st.from_user=$1 OR st.to_user=$1)
       WHERE u.id = $1
       GROUP BY u.id, e.category, e.currency_id;
        ",user.id, group_id, from_time).fetch_all(pool).await?;
        let mut categorised_amount = Vec::new();
        for rec in data {
            if let (Some(category),Some(currency_id)) = (rec.category,rec.currency_id){
                categorised_amount.push(
                    CategorisedAmount { category, amount: Amount { amount: rec.total_spent.unwrap_or_default(), currency_id } }
                );
            }
        }
        Ok(categorised_amount)
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
        let from_time = from_time.unwrap_or(chrono::Utc::now().to_rfc3339());
        let direct_group =
            Group::find_group_for_users(vec![user_1.to_string(), user_2.to_string()], pool).await;
        log::info!("Found group with user");
        if let Ok(direct_group) = direct_group {
            if direct_group.name.is_none() {
                log::info!("Using direct group with user");

                let expenses = sqlx::query_as!(
                    Expense,
                    r#"
                    WITH expense_users AS (
                        SELECT e.id, e.title, e.created_at, e.created_by, e.group_id, e.amount, e.currency_id, e.category, e.note, e.image_id
                        FROM expenses e
                        JOIN split_transactions s ON e.id = s.expense_id
                        WHERE ((s.from_user = $1 AND s.to_user = $2) OR (s.from_user = $2 AND s.to_user = $1))
                    ),
                    expense_group AS (
                        SELECT e.id, e.title, e.created_at, e.created_by, e.group_id, e.amount, e.currency_id, e.category, e.note, e.image_id
                        FROM expenses e
                        WHERE e.group_id = $5
                    ),
                    all_expenses AS (
                        SELECT id, title, created_at, created_by, group_id, amount, currency_id, category, note, image_id
                        FROM expense_users
                        UNION ALL
                        SELECT id, title, created_at, created_by, group_id, amount, currency_id, category, note, image_id
                        FROM expense_group
                    )
                    SELECT *
                    FROM all_expenses
                    WHERE created_at < $4
                    ORDER BY created_at DESC
                    LIMIT $3
                    "#,
                    user_1,
                    user_2,
                    limit,
                    from_time,
                    direct_group.id,
                )
                .fetch_all(pool)
                .await?;
                return Ok(expenses);
            }
        }
        let expenses = sqlx::query_as!(
                Expense,
                r#"
    SELECT e.id, e.title, e.created_at as created_at, e.created_by, e.group_id, e.amount, e.currency_id, e.category, e.note, e.image_id
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
            .await?;

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

#[derive(SimpleObject)]
pub struct ExpenseMixSplit {
    pub expense: Option<Expense>,
    pub split: Option<Split>,
}

#[derive(SimpleObject)]
pub struct CategorisedAmount{
    pub category: String,
    pub amount: Amount,
}