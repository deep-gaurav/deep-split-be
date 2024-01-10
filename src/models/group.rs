use std::collections::HashMap;

use anyhow::Ok;
use async_graphql::{Context, Object, SimpleObject};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{auth::AuthTypes, schema::get_pool_from_context};

use super::{
    amount::Amount,
    expense::Expense,
    split::{Split, TransactionType},
    user::User,
};

#[derive(Debug, sqlx::FromRow)]
pub struct Group {
    pub id: String,
    pub name: Option<String>,
    pub created_at: String,
    pub creator_id: String,
}

#[derive(SimpleObject)]
pub struct GroupMember {
    pub member: User,
    pub owed_in_group: Vec<Amount>,
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

    pub async fn members<'ctx>(&self, context: &Context<'ctx>) -> anyhow::Result<Vec<GroupMember>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        self.get_group_members(&user.id, pool).await
    }

    pub async fn expenses<'ctx>(
        &self,
        context: &Context<'ctx>,
        from_time: Option<String>,
        #[graphql(default = 10)] limit: u32,
    ) -> anyhow::Result<Vec<Expense>> {
        let pool = get_pool_from_context(context).await?;
        self.get_expenses(limit, from_time, pool).await
    }
}

impl Group {
    pub async fn find_group_for_users(
        users: Vec<String>,
        pool: &SqlitePool,
    ) -> anyhow::Result<Group> {
        let users_count = users.len().to_string();
        // let users = users.join(",");

        //TODO: stuck due to https://github.com/launchbadge/sqlx/issues/875
        let in_string = (1..=users.len())
            .map(|i| format!("${}", i))
            .collect::<Vec<_>>()
            .join(", ");

        log::info!("User Count {users_count}");
        let query_string = r##"
        SELECT g.*
        FROM groups g
        JOIN group_memberships gm ON g.id = gm.group_id
        WHERE g.name IS NULL AND gm.user_id IN ({QUERY_IN})
        GROUP BY g.id
        HAVING COUNT(DISTINCT gm.user_id) = ${END_BIND}
        AND COUNT(DISTINCT gm.user_id) = (SELECT COUNT(*) FROM users WHERE id IN ({QUERY_IN}))
        "##
        .replace("{QUERY_IN}", &in_string)
        .replace("{END_BIND}", (users.len() + 1).to_string().as_str());
        log::info!("Querying {query_string}");
        let mut query = sqlx::query_as::<_, Group>(&query_string);
        for user in users.iter() {
            query = query.bind(user);
        }
        query = query.bind(users.len() as i64);

        let group = query.fetch_one(pool).await?;
        Ok(group)
    }
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

    pub async fn get_users(group_id: &str, pool: &SqlitePool) -> anyhow::Result<Vec<User>> {
        let users = sqlx::query_as!(
            User,
            r#"
            SELECT users.* FROM 
                users JOIN group_memberships ON users.id=group_memberships.user_id 
                JOIN groups ON group_memberships.group_id=groups.id AND groups.id=$1
            "#,
            group_id
        )
        .fetch_all(pool)
        .await?;
        Ok(users)
    }

    pub async fn get_group_members(
        &self,
        user_id: &str,
        pool: &SqlitePool,
    ) -> anyhow::Result<Vec<GroupMember>> {
        let orig_users = Self::get_users(&self.id, pool).await?;
        let mut grouped = HashMap::new();
        sqlx::query!(
            r#"
        SELECT 
            u.id AS user_id,
            u.name AS user_name,
            u.phone AS user_phone,
            u.email AS user_email,
            u.notification_token AS user_notification_token,
            st.currency_id AS currency,
            SUM(CASE WHEN st.from_user = $1 AND st.to_user = u.id THEN st.amount ELSE 0 END) - 
            SUM(CASE WHEN st.to_user = $1 AND st.from_user = u.id THEN st.amount ELSE 0 END) AS owed_amount
        FROM 
            group_memberships m
        INNER JOIN 
            split_transactions st ON m.group_id = st.group_id
        INNER JOIN 
            users u ON u.id = m.user_id
        WHERE 
            m.group_id = $2
        GROUP BY 
            u.id, u.name, u.phone, u.email, u.notification_token, st.currency_id;
            "#,
            user_id,
            self.id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .for_each( |record| {
            grouped.entry(record.user_id.clone()).or_insert_with(||(User {
                id: record.user_id,
                name: record.user_name,
                phone: record.user_phone,
                email: record.user_email,
                notification_token: record.user_notification_token,
            },Vec::new()))
            .1.push(Amount{
                amount:record.owed_amount as i64,
                currency_id:record.currency,
            })
        });
        let users_combined = orig_users
            .into_iter()
            .map(|u| GroupMember {
                owed_in_group: grouped
                    .get(u.id.as_str())
                    .map(|val| val.1.clone())
                    .unwrap_or_default(),
                member: u,
            })
            .collect();

        Ok(users_combined)
    }

    pub async fn get_expenses(
        &self,
        limit: u32,
        from_time: Option<String>,
        pool: &SqlitePool,
    ) -> anyhow::Result<Vec<Expense>> {
        let expenses = if let Some(from_time) = from_time {
            sqlx::query_as!(
                Expense,
                r#"SELECT 
                id as "id!", title as  "title!", amount as "amount!", created_at as "created_at!", group_id as "group_id!", created_by as "created_by!", currency_id as "currency_id!" 
                FROM expenses where group_id=$1 AND created_at<$3 ORDER BY created_at DESC LIMIT $2"#,
                self.id,
                limit,
                from_time
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                Expense,
                r#"SELECT 
                id as "id!", title as  "title!", amount as "amount!", created_at as "created_at!", group_id as "group_id!", created_by as "created_by!", currency_id as "currency_id!" 
                FROM expenses where group_id=$1 ORDER BY created_at DESC LIMIT $2"#,
                self.id,
                limit,
            )
            .fetch_all(pool)
            .await?
        };
        Ok(expenses)
    }

    pub async fn settle_for_group<'a>(
        group_id: &str,
        from_user: &str,
        to_user: &str,
        amount: i64,
        creator_id: &str,
        part_transaction: Option<String>,
        transaction_type: TransactionType,
        transaction: &mut sqlx::Transaction<'a, sqlx::Sqlite>,
        with_group_id: Option<String>,
        currency_id: &str,
    ) -> anyhow::Result<Split> {
        let id = Uuid::new_v4().to_string();
        let time = chrono::Utc::now().to_rfc3339();
        let ttype = transaction_type.to_string();
        let split = sqlx::query_as!(
            Split,
            "
            INSERT INTO split_transactions(
                id,
                amount,
                from_user,
                to_user,
                transaction_type,
                part_transaction,
                created_at,
                created_by,
                group_id,
                with_group_id,
                currency_id
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10,
                $11
            )
             RETURNING * 
            ",
            id,
            amount,
            from_user,
            to_user,
            ttype,
            part_transaction,
            time,
            creator_id,
            group_id,
            with_group_id,
            currency_id
        )
        .fetch_one(transaction.as_mut())
        .await?;
        Ok(split)
    }
}
