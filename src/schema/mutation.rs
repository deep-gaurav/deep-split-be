use async_graphql::{Context, InputObject, Object, SimpleObject};
use futures::{stream::FuturesUnordered, StreamExt};
use rand::Rng;
use sqlx::{Pool, Sqlite};
use tokio::sync::RwLock;

use crate::{
    auth::{create_tokens, decode_refresh_token, AuthResult, AuthTypes, UserSignedUp},
    email::{send_email_invite, send_email_otp},
    expire_map::ExpiringHashMap,
    models::{expense::Expense, group::Group, user::User},
};

use super::get_pool_from_context;

pub type OtpMap = RwLock<ExpiringHashMap<String, String>>;

#[derive(Debug, SimpleObject)]
pub struct SignupSuccess {
    pub user: User,
    pub tokens: UserSignedUp,
}

#[derive(SimpleObject)]
pub struct NonGroupExpense {
    pub group: Group,
    pub expense: Expense,
}

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
            let mut otp_map = otp_map.write().await;
            if let Some(otp) = otp_map.get(&email) {
                otp.to_string()
            } else {
                let mut rng = rand::thread_rng();
                let random_number: u32 = rng.gen_range(0..1_000_000);
                let random_string = format!("{:06}", random_number);
                random_string
            }
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
                    otp_map.remove(&email);
                    break 'otp true;
                }
            }
            false
        };
        if correct_otp {
            let user = User::get_from_email(&email, pool).await;
            match user {
                Ok(user) => {
                    if user.name.is_none() {
                        create_tokens(None, user.email, user.phone)
                    } else {
                        create_tokens(Some(user.id), user.email, user.phone)
                    }
                }
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
    ) -> anyhow::Result<SignupSuccess> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(claims) => {
                let pool = get_pool_from_context(context).await?;
                let email = claims
                    .email
                    .clone()
                    .ok_or(anyhow::anyhow!("Only email is supported now"))?;
                let user = match User::get_from_email(&email, pool).await {
                    Ok(user) => User::set_user_name(&user.id, &name, pool).await?,
                    Err(_) => {
                        let id = uuid::Uuid::new_v4().to_string();

                        User::new_user(
                            &id,
                            &name,
                            claims.phone_number.clone(),
                            claims.email.clone(),
                            upi_id,
                            pool,
                        )
                        .await
                        .map_err(|_e| anyhow::anyhow!("Can't create user"))?
                    }
                };

                let tokens = create_tokens(
                    Some(user.id.clone()),
                    user.email.clone(),
                    user.phone.clone(),
                )?
                .try_into_user_signed_up()
                .map_err(|_er| {
                    anyhow::anyhow!("Token generation failed, user signup successfull")
                })?;
                Ok(SignupSuccess { user, tokens })
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
                let group = Group::create_group(&id, &_user.id, Some(name), pool)
                    .await
                    .map_err(|_e| anyhow::anyhow!("Can't create group"))?;
                Ok(group)
            }
        }
    }

    pub async fn add_to_group_by_email<'ctx>(
        &self,
        context: &Context<'ctx>,
        group_id: String,
        email: String,
    ) -> anyhow::Result<&str> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(_user) => {
                let Some(name) = _user.name.clone() else {
                    return Err(anyhow::anyhow!("wtf??"));
                };

                let pool = get_pool_from_context(context).await?;
                let user_groups = _user.get_groups(pool).await?;
                if user_groups.iter().any(|group| group.id == group_id) {
                    let user = User::get_from_email(&email, pool).await;
                    let user = match user {
                        Ok(user) => user,
                        Err(_) => {
                            let id = uuid::Uuid::new_v4().to_string();
                            let user = User::new_invite_user(&id, email.to_string(), pool).await?;
                            let _ = send_email_invite(&email, &name).await;
                            user
                        }
                    };

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

    pub async fn add_non_group_expense<'ctx>(
        &self,
        context: &Context<'ctx>,
        title: String,
        amount: i64,
        splits: Vec<SplitInputNonGroup>,
    ) -> anyhow::Result<NonGroupExpense> {
        let auth_type = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        match auth_type {
            AuthTypes::UnAuthorized => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedNotSignedUp(_phone) => Err(anyhow::anyhow!("Unauthorized")),
            AuthTypes::AuthorizedUser(_user) => {
                let pool = get_pool_from_context(context).await?;
                let futures = FuturesUnordered::new();
                let Some(name) = _user.name.clone() else {
                    return Err(anyhow::anyhow!("wtf??"));
                };

                // let mut split_users = vec![];

                async fn map_split_input_group_to_user(
                    split: &SplitInputNonGroup,
                    inviter: String,
                    pool: &Pool<Sqlite>,
                ) -> anyhow::Result<SplitInput> {
                    if let Some(user_id) = &split.user_id {
                        let user = User::get_from_id(user_id, pool).await?;
                        Ok(SplitInput {
                            user_id: user.id,
                            amount: split.amount,
                        })
                    } else if let Some(email) = &split.email {
                        if let Ok(user) = User::get_from_email(email, pool).await {
                            Ok(SplitInput {
                                user_id: user.id,
                                amount: split.amount,
                            })
                        } else {
                            let id = uuid::Uuid::new_v4().to_string();
                            let user = User::new_invite_user(&id, email.to_string(), pool).await?;
                            let _ = send_email_invite(email, &inviter).await;
                            Ok(SplitInput {
                                user_id: user.id,
                                amount: split.amount,
                            })
                        }
                    } else {
                        Err(anyhow::anyhow!("User must have user id or email"))
                    }
                }
                for split in splits.iter() {
                    futures.push(map_split_input_group_to_user(split, name.clone(), pool))
                }

                let users = futures.collect::<Vec<_>>().await;
                let mut splits = vec![];
                for user in users {
                    match user {
                        Ok(user) => splits.push(user),
                        Err(err) => return Err(anyhow::anyhow!("Can not get split user {err:?}")),
                    }
                }
                let mut user_ids = vec![_user.id.clone()];
                splits.iter().for_each(|f| user_ids.push(f.user_id.clone()));

                let group = match Group::find_group_for_users(user_ids, pool).await {
                    Ok(gid) => gid,
                    Err(_) => {
                        let id = uuid::Uuid::new_v4().to_string();
                        let group = Group::create_group(&id, &_user.id, None, pool).await?;
                        let futures = FuturesUnordered::new();
                        for user in splits.iter() {
                            futures.push(Group::add_to_group(&group.id, &user.user_id, pool))
                        }
                        let result = futures.collect::<Vec<_>>().await;
                        if result.iter().any(|v| v.is_err()) {
                            return Err(anyhow::anyhow!("Cannot add everyone to group"));
                        }
                        group
                    }
                };
                let expense = self
                    .add_expense(context, group.id.to_string(), title, amount, splits)
                    .await?;
                Ok(NonGroupExpense { group, expense })
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

#[derive(InputObject)]
pub struct SplitInputNonGroup {
    pub amount: i64,
    pub email: Option<String>,
    pub user_id: Option<String>,
}
