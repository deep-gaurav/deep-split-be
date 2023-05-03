use async_graphql::{Context, Object, SimpleObject, Union};

use crate::{auth::AuthTypes, models::user::User};

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
                Ok(UserAuth::Unregistered(Unregistered { phone: phone }))
            }
            AuthTypes::AuthorizedUser(user) => Ok(UserAuth::Registered(Registered { user })),
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
