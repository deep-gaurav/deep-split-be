use anyhow::Ok;
use async_graphql::{Context, InputObject, Object};

use crate::{
    auth::AuthTypes,
    models::{expense::Expense, group::Group, user::User},
};

use super::get_pool_from_context;

pub struct Mutation;

#[Object]
impl Mutation {
    pub async fn signup<'ctx>(
        &self,
        context: &Context<'ctx>,
        name: String,
    ) -> anyhow::Result<User> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(phone) => {
                let pool = get_pool_from_context(context).await?;
                let id = uuid::Uuid::new_v4().to_string();
                let user = User::new_user(&id, &name, phone, pool)
                    .await
                    .map_err(|_e| anyhow::anyhow!("Can't create user"))?;
                Ok(user)
            }
            AuthTypes::AuthorizedUser(_user) => Err(anyhow::anyhow!("Already Registered user")),
        }
    }

    pub async fn create_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        name: String,
    ) -> anyhow::Result<Group> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(_user) => {
                let pool = get_pool_from_context(context).await?;
                let id = uuid::Uuid::new_v4().to_string();
                let group = Group::create_group(&id, &_user.id, &name, pool)
                    .await
                    .map_err(|_e| anyhow::anyhow!("Can't create group"))?;
                Ok(group)
            }
        }
    }

    pub async fn add_to_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        group_id: String,
        phone: String,
    ) -> anyhow::Result<&str> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(_user) => {
                let pool = get_pool_from_context(context).await?;
                let user_groups = _user.get_groups(pool).await?;
                if user_groups.iter().any(|group| group.id == group_id) {
                    let user = User::get_from_phone(&phone, pool)
                        .await
                        .map_err(|_| anyhow::anyhow!("No user with given phone"))?;
                    Group::add_to_group(&group_id, &user.id, pool)
                        .await
                        .map_err(|_e| anyhow::anyhow!("Can't create group"))?;
                    Ok("success")
                } else {
                    Err(anyhow::anyhow!("You must be in group to add other user"))
                }
            }
        }
    }

    pub async fn add_expense<'ctx>(
        &self,
        context: &Context<'ctx>,
        group_id: String,
        title: String,
        amount: f64,
        splits: Vec<SplitInput>,
    ) -> anyhow::Result<Expense> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(_user) => {
                if splits.iter().any(|split| split.user_id == _user.id) {
                    return Err(anyhow::anyhow!("Cant split to self"));
                }
                let pool = get_pool_from_context(context).await?;
                let expense =
                    Expense::new_expense(&_user.id, &title, &group_id, amount, splits, pool)
                        .await?;
                Ok(expense)
            }
        }
    }
}

#[derive(InputObject)]
pub struct SplitInput {
    pub amount: i64,
    pub user_id: String,
}
