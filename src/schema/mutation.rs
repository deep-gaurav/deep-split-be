use std::collections::HashMap;

use async_graphql::{Context, InputObject, Object, SimpleObject};
use futures::{stream::FuturesUnordered, StreamExt};
use ip2country::AsnDB;
use rand::Rng;
use sqlx::{Pool, Sqlite};
use tokio::sync::RwLock;

use crate::{
    auth::{
        create_tokens, decode_refresh_token, AuthResult, AuthTypes, ForwardedHeader, UserSignedUp,
    },
    email::{send_email_invite, send_email_otp},
    expire_map::ExpiringHashMap,
    models::{
        amount::Amount,
        expense::Expense,
        group::Group,
        split::{Split, TransactionType},
        user::{User, UserConfig},
    },
};

use super::{currency_from_ip, get_pool_from_context};

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
                    Ok(user) => User::set_user_name(&user.id, &name, pool).await?,
                    Err(_) => {
                        let id = uuid::Uuid::new_v4().to_string();

                        User::new_user(
                            &id,
                            &name,
                            claims.phone_number.clone(),
                            claims.email.clone(),
                            upi_id,
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
        currency_id: String,
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
                    )
                    .await?;
                for user in splits.into_iter() {
                    let _ = self.simplify_cross_group(context, user.user_id).await;
                }

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
        currency_id: String,
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
                if amount <= 0 {
                    return Err(anyhow::anyhow!("Amount must be greater than 0"));
                }
                let pool = get_pool_from_context(context).await?;
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
                    &title,
                    &group_id,
                    &Amount {
                        amount,
                        currency_id,
                    },
                    splits.clone(),
                    pool,
                )
                .await?;
                for user in splits.into_iter() {
                    let _ = self.simplify_cross_group(context, user.user_id).await;
                }
                Ok(expense)
            }
        }
    }

    pub async fn settle_in_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        to_user: String,
        group_id: String,
        amount: i64,
        currency_id: String,
    ) -> anyhow::Result<Split> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
        let members = Group::get_users(&group_id, pool).await?;
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
        )
        .await?;
        transaction.commit().await?;
        let _ = self.simplify_cross_group(context, to_user).await;
        Ok(split)
    }

    pub async fn simplify_cross_group<'ctx>(
        &self,
        context: &Context<'ctx>,
        with_user: String,
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
                                &currency,
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
                                &currency,
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
        with_user: String,
        amount: i64,
        currency_id: String,
    ) -> anyhow::Result<Vec<Split>> {
        let self_user = context
            .data::<AuthTypes>()
            .map_err(|e| anyhow::anyhow!("{e:#?}"))?
            .as_authorized_user()
            .ok_or(anyhow::anyhow!("Unauthorized"))?;
        let pool = get_pool_from_context(context).await?;
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
                    )
                    .await?,
                )
            }
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
                        futures.push(Group::add_to_group(&group.id, user_id.as_str(), pool))
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
                )
                .await?,
            );
            transaction.commit().await?;
        }
        let _ = self.simplify_cross_group(context, with_user).await;

        Ok(splits)
    }

    pub async fn set_default_currency<'ctx>(
        &self,
        context: &Context<'ctx>,
        currency_id: String,
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

    // pub async fn settle_expense<'ctx>(
    //     &self,
    //     context: &Context<'ctx>,
    //     expense_id: String,
    //     amount: i64,
    // ) -> anyhow::Result<Expense> {
    //     let _user = context
    //         .data::<AuthTypes>()
    //         .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //         .as_authorized_user()
    //         .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
    //     let pool = get_pool_from_context(context).await?;

    //     Expense::settle_expense(&expense_id, &_user.id, amount, pool).await?;
    //     let expense = Expense::get_from_id(&expense_id, pool).await?;

    //     Ok(expense)
    // }

    //     pub async fn settle_user<'ctx>(
    //         &self,
    //         context: &Context<'ctx>,
    //         user_id: String,
    //         amount: i64,
    //     ) -> anyhow::Result<&str> {
    //         let _user = context
    //             .data::<AuthTypes>()
    //             .map_err(|e| anyhow::anyhow!("{e:#?}"))?
    //             .as_authorized_user()
    //             .ok_or_else(|| anyhow::anyhow!("Unauthorized"))?;
    //         let pool = get_pool_from_context(context).await?;

    //         _user.settle_expense(&user_id, amount, pool).await?;
    //         Ok("success")
    //     }
}

#[derive(InputObject, Clone)]
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
