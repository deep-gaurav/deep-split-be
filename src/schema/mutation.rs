use std::collections::HashMap;

use crate::{
    models::user::PaymentMode, notification::send_message_notification_with_retry, s3::S3,
};
use async_graphql::{Context, InputObject, Object, SimpleObject};
use futures::{stream::FuturesUnordered, StreamExt};
use ip2country::AsnDB;

use rand::Rng;
use sqlx::{Pool, Sqlite};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    auth::{
        create_tokens, decode_refresh_token, AuthResult, AuthTypes, ForwardedHeader, UserSignedUp,
    },
    email::{send_email_invite, send_email_otp},
    expire_map::ExpiringHashMap,
    models::{
        amount::Amount,
        currency::Currency,
        expense::Expense,
        group::Group,
        split::{Split, TransactionType},
        user::{User, UserConfig},
    },
};

use super::{
    currency_from_ip, get_pool_from_context, DateTimeValidator, IdValidator, NameValidator,
    UpiIdValidator,
};

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
        #[graphql(validator(email))] email: String,
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
        #[graphql(validator(email))] email: String,
        #[graphql(validator(max_length = 6))] otp: String,
    ) -> anyhow::Result<AuthResult> {
        let pool = get_pool_from_context(context).await?;

        let otp_map = context
            .data::<OtpMap>()
            .map_err(|_e| anyhow::anyhow!("Something went wrong"))?;
        let correct_otp = 'otp: {
            if email == "guest@billdivide.app" && &otp == "123456" {
                break 'otp true;
            };
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

    pub async fn refresh_token(
        &self,
        #[graphql(validator(max_length = 8000))] refresh_token: String,
    ) -> anyhow::Result<UserSignedUp> {
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
        #[graphql(validator(
            custom = r#"NameValidator::new("name")"#,
            min_length = 3,
            max_length = 20
        ))]
        name: String,
    ) -> anyhow::Result<SignupSuccess> {
        let name = name.trim();
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
                let currency_id = 'currency: {
                    let Ok(db) = context.data::<AsnDB>() else {
                        break 'currency "USD".to_string();
                    };
                    let Some(header) = context.data_opt::<ForwardedHeader>() else {
                        break 'currency "USD".to_string();
                    };
                    let Ok(currency) = currency_from_ip(pool, header, db).await else {
                        break 'currency "USD".to_string();
                    };
                    currency.id
                };
                let user = match User::get_from_email(&email, pool).await {
                    Ok(user) => User::set_user_name(&user.id, name, currency_id, pool).await?,
                    Err(_) => {
                        let id = uuid::Uuid::new_v4().to_string();

                        User::new_user(
                            &id,
                            name,
                            claims.phone_number.clone(),
                            claims.email.clone(),
                            currency_id,
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
        #[graphql(validator(
            custom = r#"NameValidator::new("name")"#,
            min_length = 3,
            max_length = 20
        ))]
        name: String,
    ) -> anyhow::Result<Group> {
        let name = name.trim().to_string();
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

    pub async fn set_notification_token<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(max_length = 8000))] token: String,
    ) -> anyhow::Result<String> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        sqlx::query!(
            "UPDATE users SET notification_token = $1 WHERE id = $2",
            token,
            self_user.id
        )
        .execute(pool)
        .await?;
        Ok("success".to_string())
    }

    pub async fn add_to_group_by_email<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("group_id")"#))] group_id: String,
        #[graphql(validator(email))] email: String,
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
                let group = Group::get_from_id(&group_id, pool).await?;
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
                    if let Some(token) = user.notification_token {
                        if let Err(err) = send_message_notification_with_retry(
                            format!(
                                "{} added you to group {}",
                                _user.name.as_ref().unwrap_or(&"Someone".to_string()),
                                group.name.as_ref().unwrap_or(&"Direct Payment".to_string()),
                            )
                            .as_str(),
                            "/",
                            "https://billdivide.app/",
                            format!(
                                "you were added to group {} by {}",
                                group.name.as_ref().unwrap_or(&"Direct Payment".to_string()),
                                _user.name.as_ref().unwrap_or(&"Someone".to_string()),
                            )
                            .as_str(),
                            &token,
                            None,
                        )
                        .await
                        {
                            log::warn!("Failed to send notification {err:?}")
                        } else {
                            log::info!("Notification sent")
                        }
                    } else {
                        log::info!("Skipping notification, no token")
                    }
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

    #[allow(clippy::too_many_arguments)]
    pub async fn add_non_group_expense<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(
            custom = r#"NameValidator::new("title")"#,
            min_length = 3,
            max_length = 20
        ))]
        title: String,
        amount: i64,
        #[graphql(validator(max_length = 100))] currency_id: String,
        splits: Vec<SplitInputNonGroup>,
        #[graphql(validator(max_length = 300))] note: Option<String>,
        #[graphql(validator(custom = r#"IdValidator::new("image_id")"#))] image_id: Option<String>,
        #[graphql(default = "\"MISC\".to_string()", validator(max_length = 100))] category: String,
        #[graphql(validator(custom = r#"DateTimeValidator::new("transaction_at")"#))]
        transaction_at: Option<String>,
    ) -> anyhow::Result<NonGroupExpense> {
        let title = title.trim().to_string();
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

                log::info!("Searching for groups with users {user_ids:?}");
                let group = match Group::find_group_for_users(user_ids, pool).await {
                    Ok(gid) => {
                        log::info!("Found existing group {gid:?}");
                        gid
                    }
                    Err(err) => {
                        log::info!("Not found existing group {err:?}");
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
                    .add_expense(
                        context,
                        group.id.to_string(),
                        title,
                        amount,
                        currency_id,
                        splits.clone(),
                        note,
                        image_id,
                        category,
                        transaction_at,
                    )
                    .await?;
                for user in splits.into_iter() {
                    let _ = self.simplify_cross_group(context, user.user_id).await;
                }

                Ok(NonGroupExpense { group, expense })
            }
        }
    }

    // pub async fn edit_expense_title<'ctx>(
    //     &self,
    //     context: &Context<'ctx>,
    //     #[graphql(validator(max_length = 100))] expense_id: String,
    //     #[graphql(validator(
    //         regex = r"^[^\p{P}\p{S}\p{C}0-9_]+(?: [^\p{P}\p{S}\p{C}0-9_]+)*$",
    //         min_length = 3,
    //         max_length = 20
    //     ))]
    //     title: String,
    // ) -> anyhow::Result<Expense> {
    //     let _self_user = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //         .as_authorized_user()
    //         .ok_or(anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;
    //     let update_time = chrono::Utc::now().to_rfc3339();

    //     let result = sqlx::query_as!(
    //         Expense,
    //         "UPDATE expenses SET title=$1, updated_at=$3 WHERE id=$2 RETURNING *",
    //         title,
    //         expense_id,
    //         update_time
    //     )
    //     .fetch_one(pool)
    //     .await?;
    //     Ok(result)
    // }

    // pub async fn edit_expense_category<'ctx>(
    //     &self,
    //     context: &Context<'ctx>,
    //     #[graphql(validator(max_length = 100))] expense_id: String,
    //     #[graphql(default = "\"MISC\".to_string()", validator(max_length = 100))] category: String,
    // ) -> anyhow::Result<Expense> {
    //     let _self_user = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //         .as_authorized_user()
    //         .ok_or(anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;
    //     let update_time = chrono::Utc::now().to_rfc3339();

    //     let result = sqlx::query_as!(
    //         Expense,
    //         "UPDATE expenses SET category=$1, updated_at=$3 WHERE id=$2 RETURNING *",
    //         category,
    //         expense_id,
    //         update_time
    //     )
    //     .fetch_one(pool)
    //     .await?;
    //     Ok(result)
    // }

    // pub async fn edit_expense<'ctx>(
    //     &self,
    //     context: &Context<'ctx>,
    //     #[graphql(validator(max_length = 100))] expense_id: String,
    //     #[graphql(validator(
    //         regex = r"^[^\p{P}\p{S}\p{C}0-9_]+(?: [^\p{P}\p{S}\p{C}0-9_]+)*$",
    //         min_length = 3,
    //         max_length = 20
    //     ))]
    //     title: Option<String>,
    //     amount: Option<i64>,
    //     #[graphql(validator(max_length = 100))] currency_id: Option<String>,
    //     splits: Option<Vec<SplitInput>>,
    //     #[graphql(validator(max_length = 300))] note: Option<String>,
    //     #[graphql(validator(max_length = 100))] image_id: Option<String>,
    //     #[graphql(default = "\"MISC\".to_string()", validator(max_length = 100))] category: String,
    //     #[graphql(default)] should_replace_image: bool,
    // ) -> anyhow::Result<Expense> {
    //     let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    //     let self_user = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //         .as_authorized_user()
    //         .ok_or(anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;

    //     let expense = Expense::get_from_id(&expense_id, pool).await?;
    //     if expense.created_by == self_user.id {
    //         return Err(anyhow::anyhow!("You are not creator"));
    //     }
    //     let mut transaction = pool.begin().await?;
    //     if let Some(title) = title {
    //         let result = sqlx::query!(
    //             "UPDATE expenses SET title=$1 WHERE id=$2",
    //             title,
    //             expense_id
    //         )
    //         .execute(transaction.as_mut())
    //         .await?
    //         .rows_affected();
    //         if result != 1 {
    //             return Err(anyhow::anyhow!("Something went wrong"));
    //         }
    //     }
    //     if let Some(amount) = amount {
    //         let (Some(splits), Some(currency_id)) = (splits, currency_id) else {
    //             return Err(anyhow::anyhow!(
    //                 "Must have split with amount and currency id"
    //             ));
    //         };
    //         if splits.iter().any(|split| split.user_id == self_user.id) {
    //             return Err(anyhow::anyhow!("Cant split to self"));
    //         }
    //         if amount <= 0 {
    //             return Err(anyhow::anyhow!("Amount must be greater than 0"));
    //         }
    //         let splits = splits
    //             .into_iter()
    //             .filter(|f| f.amount > 0)
    //             .collect::<Vec<_>>();
    //         let group_members = Group::get_users(&expense.group_id, pool).await?;
    //         if !splits
    //             .iter()
    //             .all(|s| group_members.iter().any(|user| user.id == s.user_id))
    //         {
    //             return Err(anyhow::anyhow!("Not everyone is group member"));
    //         }
    //         let result = sqlx::query!(
    //             "UPDATE expenses SET amount=$1, currency_id=$2 WHERE id=$3",
    //             amount,
    //             currency_id,
    //             expense_id
    //         )
    //         .execute(transaction.as_mut())
    //         .await?
    //         .rows_affected();
    //         Expense::edit_expense_splits(&expense_id, splits, &self_user.id, s3, &mut transaction)
    //             .await?;
    //         if result != 1 {
    //             return Err(anyhow::anyhow!("Something went wrong"));
    //         }
    //     }
    //     todo!()
    // }
    #[allow(clippy::too_many_arguments)]
    pub async fn add_expense<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("group_id")"#))] group_id: String,
        #[graphql(validator(
            custom = r#"NameValidator::new("title")"#,
            min_length = 3,
            max_length = 20
        ))]
        title: String,
        amount: i64,
        #[graphql(validator(max_length = 100))] currency_id: String,
        splits: Vec<SplitInput>,
        #[graphql(validator(max_length = 300))] note: Option<String>,
        #[graphql(validator(custom = r#"IdValidator::new("group_id")"#))] image_id: Option<String>,
        #[graphql(default = "\"MISC\".to_string()", validator(max_length = 100))] category: String,
        #[graphql(validator(custom = r#"DateTimeValidator::new("transaction_at")"#))]
        transaction_at: Option<String>,
    ) -> anyhow::Result<Expense> {
        let title = title.trim();
        let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;

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
                if amount <= 0 {
                    return Err(anyhow::anyhow!("Amount must be greater than 0"));
                }
                let pool = get_pool_from_context(context).await?;
                let currency = Currency::get_for_id(pool, &currency_id).await?;
                let group = Group::get_from_id(&group_id, pool).await?;
                let splits = splits
                    .into_iter()
                    .filter(|f| f.amount > 0)
                    .collect::<Vec<_>>();
                let group_members = Group::get_users(&group_id, pool).await?;
                if !splits
                    .iter()
                    .all(|s| group_members.iter().any(|user| user.id == s.user_id))
                {
                    return Err(anyhow::anyhow!("Not everyone is group member"));
                }
                let expense = Expense::new_expense(
                    &_user.id,
                    title,
                    &group_id,
                    &Amount {
                        amount,
                        currency_id,
                    },
                    splits.clone(),
                    &category,
                    note,
                    image_id,
                    transaction_at,
                    s3,
                    pool,
                )
                .await?;
                for split in splits.iter() {
                    let to_user_model = User::get_from_id(&split.user_id, pool).await;
                    if let Ok(to_user_model) = to_user_model {
                        if let Some(token) = to_user_model.notification_token {
                            if let Err(err) = send_message_notification_with_retry(
                                format!(
                                    "{} added expense {}",
                                    _user.name.as_ref().unwrap_or(&"Someone".to_string()),
                                    title,
                                )
                                .as_str(),
                                "/",
                                "https://billdivide.app/",
                                format!(
                                    "you owe {}{} to {} in group {}",
                                    currency.symbol,
                                    ((split.amount as f64) / 10_f64.powi(currency.decimals as i32))
                                        as i64,
                                    _user.name.as_ref().unwrap_or(&"Someone".to_string()),
                                    group.name.as_ref().unwrap_or(&"Direct Payment".to_string())
                                )
                                .as_str(),
                                &token,
                                Some("new_expense"),
                            )
                            .await
                            {
                                log::warn!("Failed to send notification {err:?}")
                            } else {
                                log::info!("Notification sent")
                            }
                        } else {
                            log::info!("Skipping notification, no token")
                        }
                    }
                }
                for user in splits.into_iter() {
                    let _ = self.simplify_cross_group(context, user.user_id).await;
                }

                Ok(expense)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn settle_in_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("to_user")"#))] to_user: String,
        #[graphql(validator(custom = r#"IdValidator::new("group_id")"#))] group_id: String,
        amount: i64,
        #[graphql(validator(max_length = 100))] currency_id: String,
        #[graphql(validator(custom = r#"IdValidator::new("image_id")"#))] image_id: Option<String>,
        #[graphql(validator(max_length = 300))] note: Option<String>,
        #[graphql(validator(max_length = 1000))] transaction_metadata: Option<String>,
    ) -> anyhow::Result<Split> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let group = Group::get_from_id(&group_id, pool).await?;
        let members = Group::get_users(&group_id, pool).await?;
        let currency = Currency::get_for_id(pool, &currency_id).await?;
        let to_user_model = User::get_from_id(&to_user, pool).await?;
        if !members.iter().any(|user| user.id == to_user)
            || !members.iter().any(|user| user.id == self_user.id)
        {
            return Err(anyhow::anyhow!("Cant settle to non members"));
        }
        let mut transaction = pool.begin().await?;
        let split = Group::settle_for_group(
            &group_id,
            &to_user,
            &self_user.id,
            amount,
            &self_user.id,
            None,
            TransactionType::CashPaid,
            &mut transaction,
            None,
            &currency_id,
            note,
            image_id.clone(),
            transaction_metadata,
        )
        .await?;
        if let Some(image_id) = image_id {
            s3.move_to_be(&image_id).await?;
        }
        transaction.commit().await?;
        let _ = self.simplify_cross_group(context, to_user).await;
        if let Some(token) = to_user_model.notification_token {
            if let Err(err) = send_message_notification_with_retry(
                format!(
                    "{} paid you {}{}",
                    self_user.name.as_ref().unwrap_or(&"Someone".to_string()),
                    currency.symbol,
                    ((amount as f64) / 10_f64.powi(currency.decimals as i32)) as i64
                )
                .as_str(),
                "/",
                "https://billdivide.app/",
                format!(
                    "{} recorded payment of {}{} to you in group {}",
                    self_user.name.as_ref().unwrap_or(&"Someone".to_string()),
                    currency.symbol,
                    ((amount as f64) / 10_f64.powi(currency.decimals as i32)) as i64,
                    group.name.unwrap_or("Direct Payment".to_string())
                )
                .as_str(),
                &token,
                Some("new_payment"),
            )
            .await
            {
                log::warn!("Failed to send notification {err:?}")
            } else {
                log::info!("Notification sent")
            }
        } else {
            log::info!("Skipping notification, no token")
        }
        Ok(split)
    }

    pub async fn simplify_cross_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("with_user")"#))] with_user: String,
    ) -> anyhow::Result<Vec<Split>> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let owes = User::get_owes_with_group(&self_user.id, &with_user, pool).await?;
        let mut grouped_positives = HashMap::new();
        for owe in owes {
            grouped_positives
                .entry(owe.amount.currency_id.clone())
                .or_insert_with(Vec::new)
                .push((owe.amount.amount, owe.group_id));
        }
        let mut transaction = pool.begin().await?;
        let mut splits = vec![];

        for (currency, owes) in grouped_positives.iter() {
            let positives = owes.iter().filter(|ow| ow.0 > 0);
            let mut negatives = owes.iter().filter(|ow| ow.0 < 0);
            let mut negative = negatives.next();
            let mut negative_settled = 0_i64;
            let part_id = uuid::Uuid::new_v4().to_string();
            'po: for positive in positives {
                log::info!("Positive: {positive:?}");
                let mut remaining_positive = positive.0;
                while let Some(negative_val) = negative.or_else(|| negatives.next()) {
                    negative = Some(negative_val);
                    log::info!("Negative: {negative:?}");
                    let remaining_negative = negative_val.0.abs() - negative_settled;
                    if remaining_negative > 0 {
                        let (amt_settle, is_neg) = if remaining_negative > remaining_positive {
                            negative_settled += remaining_positive;
                            (remaining_positive, true)
                        } else {
                            remaining_positive -= remaining_negative;
                            (remaining_negative, false)
                        };
                        log::info!("Amount settle {amt_settle} is_neg {is_neg}");
                        splits.push(
                            Group::settle_for_group(
                                &positive.1,
                                &self_user.id,
                                &with_user,
                                amt_settle,
                                &self_user.id,
                                Some(part_id.clone()),
                                TransactionType::CrossGroupSettlement,
                                &mut transaction,
                                Some(negative_val.1.clone()),
                                currency,
                                None,
                                None,
                                None,
                            )
                            .await?,
                        );
                        splits.push(
                            Group::settle_for_group(
                                &negative_val.1,
                                &with_user,
                                &self_user.id,
                                amt_settle,
                                &self_user.id,
                                Some(part_id.clone()),
                                TransactionType::CrossGroupSettlement,
                                &mut transaction,
                                Some(positive.1.clone()),
                                currency,
                                None,
                                None,
                                None,
                            )
                            .await?,
                        );
                        if is_neg {
                            continue 'po;
                        }
                    }
                    negative = None;
                    negative_settled = 0;
                    log::info!("Next!")
                }
            }
        }

        transaction.commit().await?;

        Ok(splits)
    }

    pub async fn auto_settle_with_user<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("with_user")"#))] with_user: String,
        amount: i64,
        #[graphql(validator(max_length = 100))] currency_id: String,
        #[graphql(validator(custom = r#"IdValidator::new("image_id")"#))] image_id: Option<String>,
        #[graphql(validator(max_length = 300))] note: Option<String>,
        #[graphql(validator(max_length = 1000))] transaction_metadata: Option<String>,
    ) -> anyhow::Result<Vec<Split>> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let s3 = context.data::<S3>().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let currency = Currency::get_for_id(pool, &currency_id).await?;

        let with_user_model = User::get_from_id(&with_user, pool).await?;
        let mut owes = User::get_owes_with_group(&with_user, &self_user.id, pool)
            .await?
            .into_iter()
            .filter_map(|val| {
                if val.amount.currency_id == currency_id {
                    Some((val.group_id, val.amount.amount))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        owes.sort_by(|a, b| b.1.cmp(&a.1));
        let mut remaining_amount = amount;
        let mut splits = vec![];
        let part_id = uuid::Uuid::new_v4().to_string();

        let mut transaction = pool.begin().await?;
        for owed in owes.iter() {
            if remaining_amount <= 0 {
                break;
            }
            log::info!("Owed {owed:?}");
            if owed.1 > 0 {
                let to_pay = owed.1.min(remaining_amount);
                remaining_amount -= to_pay;
                splits.push(
                    Group::settle_for_group(
                        &owed.0,
                        &with_user,
                        &self_user.id,
                        to_pay,
                        &self_user.id,
                        Some(part_id.clone()),
                        TransactionType::CashPaid,
                        &mut transaction,
                        None,
                        &currency_id,
                        note.clone(),
                        image_id.clone(),
                        transaction_metadata.clone(),
                    )
                    .await?,
                )
            }
        }
        if let Some(image_id) = &image_id {
            s3.move_to_be(image_id).await?;
        }
        transaction.commit().await?;

        if remaining_amount > 0 {
            let user_ids = vec![self_user.id.clone(), with_user.clone()];
            let group = match Group::find_group_for_users(user_ids.clone(), pool).await {
                Ok(gid) => {
                    log::info!("Found existing group {gid:?}");
                    gid
                }
                Err(err) => {
                    log::info!("Not found existing group {err:?}");
                    let id = uuid::Uuid::new_v4().to_string();
                    let group = Group::create_group(&id, &self_user.id, None, pool).await?;
                    let futures = FuturesUnordered::new();
                    for user_id in user_ids.iter() {
                        if user_id != &self_user.id {
                            futures.push(Group::add_to_group(&group.id, user_id.as_str(), pool))
                        }
                    }
                    let result = futures.collect::<Vec<_>>().await;
                    if let Some(err) = result.iter().find(|v| v.is_err()) {
                        return Err(anyhow::anyhow!("Cannot add everyone to group {err:?}"));
                    }
                    group
                }
            };
            let mut transaction = pool.begin().await?;
            splits.push(
                Group::settle_for_group(
                    &group.id,
                    &with_user,
                    &self_user.id,
                    remaining_amount,
                    &self_user.id,
                    Some(part_id.clone()),
                    TransactionType::CashPaid,
                    &mut transaction,
                    None,
                    &currency_id,
                    note,
                    image_id.clone(),
                    transaction_metadata,
                )
                .await?,
            );
            if let Some(image_id) = image_id {
                s3.move_to_be(&image_id).await?;
            }
            transaction.commit().await?;
        }
        let _ = self.simplify_cross_group(context, with_user).await;
        if let Some(token) = with_user_model.notification_token {
            if let Err(err) = send_message_notification_with_retry(
                format!(
                    "{} paid you {}{}",
                    self_user.name.as_ref().unwrap_or(&"Someone".to_string()),
                    currency.symbol,
                    ((amount as f64) / 10_f64.powi(currency.decimals as i32)) as i64
                )
                .as_str(),
                "/",
                "https://billdivide.app/",
                format!(
                    "{} recorded payment of {}{} to you via Auto-Settlement",
                    self_user.name.as_ref().unwrap_or(&"Someone".to_string()),
                    currency.symbol,
                    ((amount as f64) / 10_f64.powi(currency.decimals as i32)) as i64
                )
                .as_str(),
                &token,
                Some("new_payment"),
            )
            .await
            {
                log::warn!("Failed to send notification {err:?}")
            } else {
                log::info!("Notification sent")
            }
        } else {
            log::info!("Skipping notification, no token")
        }
        Ok(splits)
    }

    pub async fn add_upi_id<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"UpiIdValidator::new("upi_id")"#))] upi_id: String,
    ) -> anyhow::Result<PaymentMode> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let id = Uuid::new_v4().to_string();
        let payment_mode = sqlx::query_as!(
            PaymentMode,
            "INSERT INTO payment_modes(id,mode,user_id,value) VALUES ($1,$2,$3,$4) RETURNING *",
            id,
            "UPI_VPA",
            user.id,
            upi_id
        )
        .fetch_one(pool)
        .await?;
        Ok(payment_mode)
    }

    pub async fn edit_upi_id<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("payment_mode_id")"#))]
        payment_mode_id: String,
        #[graphql(validator(custom = r#"UpiIdValidator::new("upi_id")"#))] upi_id: String,
    ) -> anyhow::Result<PaymentMode> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let previous_mode = sqlx::query_as!(
            PaymentMode,
            "SELECT * from payment_modes WHERE id=$1",
            payment_mode_id,
        )
        .fetch_one(pool)
        .await?;
        if previous_mode.user_id != user.id {
            return Err(anyhow::anyhow!("Unauthorized"));
        }
        if previous_mode.mode != "UPI_VPA" {
            return Err(anyhow::anyhow!("Not UPI_VPA mode"));
        }
        let payment_mode = sqlx::query_as!(
            PaymentMode,
            "UPDATE payment_modes SET value = $2 WHERE id=$1 RETURNING *",
            payment_mode_id,
            upi_id,
        )
        .fetch_one(pool)
        .await?;
        Ok(payment_mode)
    }

    pub async fn remove_payment_mode<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("payment_mode_id")"#))]
        payment_mode_id: String,
    ) -> anyhow::Result<PaymentMode> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let previous_mode = sqlx::query_as!(
            PaymentMode,
            "SELECT * from payment_modes WHERE id=$1",
            payment_mode_id,
        )
        .fetch_one(pool)
        .await?;

        if previous_mode.user_id != user.id {
            return Err(anyhow::anyhow!("Unauthorized"));
        }
        sqlx::query!("DELETE FROM payment_modes WHERE id = $1", payment_mode_id)
            .execute(pool)
            .await?;
        Ok(previous_mode)
    }

    pub async fn set_default_currency<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(max_length = 100))] currency_id: String,
    ) -> anyhow::Result<UserConfig> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let config = sqlx::query_as!(
            UserConfig,
            "UPDATE user_config SET default_currency_id=$1 WHERE user_id = $2 RETURNING * ",
            currency_id,
            user.id,
        )
        .fetch_one(pool)
        .await?;
        Ok(config)
    }

    pub async fn change_name<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(
            custom = r#"NameValidator::new("name")"#,
            min_length = 3,
            max_length = 20
        ))]
        name: String,
    ) -> anyhow::Result<User> {
        let name = name.trim();
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let user = sqlx::query_as!(
            User,
            "UPDATE users SET name = $1 WHERE id = $2 RETURNING *",
            name,
            user.id
        )
        .fetch_one(pool)
        .await?;
        Ok(user)
    }

    pub async fn convert_currency<'ctx>(
        &self,
        context: &Context<'ctx>,
        #[graphql(validator(custom = r#"IdValidator::new("with_user")"#))] with_user: String,
        #[graphql(validator(custom = r#"IdValidator::new("group_id")"#))] group_id: String,
        #[graphql(validator(max_length = 100))] from_currency_id: String,
        #[graphql(validator(max_length = 100))] to_currency_id: String,
    ) -> anyhow::Result<Vec<Split>> {
        let user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let owed = sqlx::query!(
            r"
            SELECT SUM(net_owed_amount) as amount FROM (
                SELECT
                    from_user,
                    to_user,
                    SUM(CASE WHEN from_user = $1 THEN amount ELSE -amount END) AS net_owed_amount
                FROM
                    split_transactions
                WHERE
                    ((from_user = $1 AND to_user = $2) OR
                    (from_user = $2 AND to_user = $1))
                    AND group_id = $3
                    AND currency_id = $4
                GROUP BY
                    from_user, to_user, group_id, currency_id
            )
            ",
            user.id,
            with_user,
            group_id,
            from_currency_id
        )
        .fetch_one(pool)
        .await?
        .amount
        .unwrap_or_default();
        match owed.cmp(&0) {
            std::cmp::Ordering::Equal => Ok(vec![]),
            std::cmp::Ordering::Greater | std::cmp::Ordering::Less => {
                let from_currency = Currency::get_for_id(pool, &from_currency_id).await?;
                let to_currency = Currency::get_for_id(pool, &to_currency_id).await?;
                let mut transaction = pool.begin().await?;
                let group_part_id = uuid::Uuid::new_v4().to_string();
                let time = chrono::Utc::now().to_rfc3339();
                let rev_id = uuid::Uuid::new_v4().to_string();
                let forw_id = uuid::Uuid::new_v4().to_string();

                let ttype = TransactionType::CurrencyConversion.to_string();

                // TODO: handle better way fails after 9,007,199,254,740,993
                // https://www.reddit.com/r/rust/comments/js1avn/comment/gbxbm2y/?utm_source=share&utm_medium=web2x&context=3
                let from_amount = owed.abs();

                let to_amount = (((owed.abs() as f64)
                    * 10_f64.powi((to_currency.decimals - from_currency.decimals) as i32))
                    / from_currency.rate
                    * to_currency.rate) as i64;

                let (from, to) = if owed.cmp(&0) == std::cmp::Ordering::Greater {
                    (&with_user, &user.id)
                } else {
                    (&user.id, &with_user)
                };

                let rev = sqlx::query_as!(
                    Split,
                    r"
                    INSERT INTO split_transactions(
                        id,
                        amount,
                        from_user,
                        to_user,
                        transaction_type,
                        part_transaction,
                        created_at,updated_at, transaction_at,
                        created_by,
                        group_id,
                        currency_id
                    )
                    VALUES (
                        $1,
                        $2,
                        $3,
                        $4,
                        $5,
                        $6,
                        $7,$7,$7,
                        $8,
                        $9,
                        $10
                    )
                     RETURNING *
                    ",
                    rev_id,
                    from_amount,
                    from,
                    to,
                    ttype,
                    group_part_id,
                    time,
                    user.id,
                    group_id,
                    from_currency_id,
                )
                .fetch_one(transaction.as_mut())
                .await?;

                let forw = sqlx::query_as!(
                    Split,
                    r"
                    INSERT INTO split_transactions(
                        id,
                        amount,
                        from_user,
                        to_user,
                        transaction_type,
                        part_transaction,
                        created_at,updated_at, transaction_at,
                        created_by,
                        group_id,
                        currency_id
                    )
                    VALUES (
                        $1,
                        $2,
                        $3,
                        $4,
                        $5,
                        $6,
                        $7,$7,$7,
                        $8,
                        $9,
                        $10
                    )
                     RETURNING *
                    ",
                    forw_id,
                    to_amount,
                    to,
                    from,
                    ttype,
                    group_part_id,
                    time,
                    user.id,
                    group_id,
                    to_currency_id,
                )
                .fetch_one(transaction.as_mut())
                .await?;
                transaction.commit().await?;
                let _ = self.simplify_cross_group(context, with_user).await;
                Ok(vec![rev, forw])
            }
        }
    }

    pub async fn upload_image<'ctx>(
        &self,
        context: &Context<'ctx>,
        size: u64,
    ) -> anyhow::Result<ImageUploadData> {
        let _user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
        if size > 1024 * 400 {
            return Err(anyhow::anyhow!("Image size too big!"));
        }
        let s3 = context
            .data::<S3>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?;
        let id = uuid::Uuid::new_v4();
        let url = s3.new_image_upload_presign_url(&id, size).await?;
        Ok(ImageUploadData {
            id: id.to_string(),
            presigned_url: url,
        })
    }
}

#[derive(InputObject, Clone)]
pub struct SplitInput {
    pub amount: i64,
    #[graphql(validator(custom = r#"IdValidator::new("user_id")"#))]
    pub user_id: String,
}

#[derive(InputObject)]
pub struct SplitInputNonGroup {
    pub amount: i64,
    #[graphql(validator(email))]
    pub email: Option<String>,
    #[graphql(validator(custom = r#"IdValidator::new("user_id")"#))]
    pub user_id: Option<String>,
}

#[derive(SimpleObject)]
pub struct ImageUploadData {
    #[graphql(validator(custom = r#"IdValidator::new("id")"#))]
    id: String,
    #[graphql(validator(max_length = 8000))]
    presigned_url: String,
}
