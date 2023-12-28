use std::{sync::Arc, todo, unimplemented};

use async_graphql::{Context, InputObject, Object, SimpleObject};
use rand::Rng;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::{
    auth::{create_tokens, decode_refresh_token, AuthResult, AuthTypes, UserSignedUp},
    email::send_email_otp,
    expire_map::ExpiringHashMap,
    models::{expense::Expense, group::Group, user::User},
};

use super::get_pool_from_context;

pub type OtpMap = RwLock<ExpiringHashMap<String, String>>;

pub struct Mutation;

#[Object]
impl Mutation {
    pub async fn send_email_otp<'ctx>(
        &self,
        context: &Context<'ctx>,
        email: String,
    ) -> anyhow::Result<bool> {
        let otp_map = context
            .data::<OtpMap>()
            .map_err(|_e| anyhow::anyhow!("Something went wrong"))?;
        let otp = {
            let mut rng = rand::thread_rng();
            let random_number: u32 = rng.gen_range(0..1_000_000);
            let random_string = format!("{:06}", random_number);
            random_string
        };
        {
            let mut otp_map = otp_map.write().await;
            otp_map.insert(email.clone(), otp.clone());
        }

        send_email_otp(&email, &otp).await?;
        Ok(true)
    }

    pub async fn verify_otp<'ctx>(
        &self,
        context: &Context<'ctx>,
        email: String,
        otp: String,
    ) -> anyhow::Result<AuthResult> {
        let pool = get_pool_from_context(context).await?;

        let otp_map = context
            .data::<OtpMap>()
            .map_err(|_e| anyhow::anyhow!("Something went wrong"))?;
        let correct_otp = 'otp: {
            let mut otp_map = otp_map.write().await;
            let correct_otp = otp_map.get(&email);
            if let Some(correct_otp) = correct_otp {
                if &otp == correct_otp {
                    break 'otp true;
                }
            }
            false
        };
        if correct_otp {
            let user = User::get_from_email(&email, pool).await;
            match user {
                Ok(user) => create_tokens(Some(user.id), user.email, user.phone),
                Err(_) => create_tokens(None, Some(email), None),
            }
        } else {
            Err(anyhow::anyhow!("OTP Mismatch or expired"))
        }
    }

    pub async fn refresh_token(&self, refresh_token: String) -> anyhow::Result<UserSignedUp> {
        let claims = decode_refresh_token(&refresh_token)?;
        let new_token = create_tokens(claims.user_id, claims.email, claims.phone_number)?;
        if let AuthResult::UserSignedUp(tokens) = new_token {
            Ok(tokens)
        } else {
            Err(anyhow::anyhow!("Refresh Token not valid"))
        }
    }

    pub async fn signup<'ctx>(
        &self,
        context: &Context<'ctx>,
        name: String,
        upi_id: Option<String>,
    ) -> anyhow::Result<User> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(claims) => {
                let pool = get_pool_from_context(context).await?;
                let id = uuid::Uuid::new_v4().to_string();
                let user = User::new_user(
                    &id,
                    &name,
                    claims.phone_number.clone(),
                    claims.email.clone(),
                    upi_id,
                    pool,
                )
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
        amount: i64,
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

    pub async fn settle_expense<'ctx>(
        &self,
        context: &Context<'ctx>,
        expense_id: String,
        amount: i64,
    ) -> anyhow::Result<Expense> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        Expense::settle_expense(&expense_id, &_user.id, amount, pool).await?;
        let expense = Expense::get_from_id(&expense_id, pool).await?;

        Ok(expense)
    }

    pub async fn settle_user<'ctx>(
        &self,
        context: &Context<'ctx>,
        user_id: String,
        amount: i64,
    ) -> anyhow::Result<&str> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;

        _user.settle_expense(&user_id, amount, pool).await?;
        Ok("success")
    }
}

#[derive(InputObject)]
pub struct SplitInput {
    pub amount: i64,
    pub user_id: String,
}
