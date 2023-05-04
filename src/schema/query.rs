use async_graphql::{Context, Object, SimpleObject, Union};

use crate::{
    auth::AuthTypes,
    models::{group::Group, user::User},
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
            AuthTypes::AuthorizedNotSignedUp(phone) => {
                Ok(UserAuth::Unregistered(Unregistered { phone }))
            }
            AuthTypes::AuthorizedUser(user) => Ok(UserAuth::Registered(Registered { user })),
        }
    }

    pub async fn group<'a>(&self, context: &Context<'a>, id: String) -> anyhow::Result<Group> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
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
}

#[derive(Union)]
pub enum UserAuth<'a> {
    Unregistered(Unregistered<'a>),
    Registered(Registered<'a>),
}

#[derive(SimpleObject)]
pub struct Unregistered<'a> {
    pub phone: &'a str,
}

#[derive(SimpleObject)]
pub struct Registered<'a> {
    pub user: &'a User,
}
