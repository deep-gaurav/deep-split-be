use async_graphql::{SimpleObject, Union};
use jsonwebtoken::{DecodingKey, EncodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::models::user::User;

#[derive(Debug)]
pub enum AuthTypes {
    UnAuthorized,
    AuthorizedNotSignedUp(Claims),
    AuthorizedUser(User),
}

impl AuthTypes {
    pub fn as_authorized_user(&self) -> Option<&User> {
        if let Self::AuthorizedUser(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Claims {
    // aud: String,         // Optional. Audience
    exp: usize, // Required (validate_exp defaults to true in validation). Expiration time (as UTC timestamp)
    // iat: usize,          // Optional. Issued at (as UTC timestamp)
    // sub: String,         // Optional. Subject (whom token refers to)
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub token_type: TokenType,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TokenType {
    Access,
    Refresh,
    Signup,
}

impl TokenType {
    /// Returns `true` if the token type is [`Access`].
    ///
    /// [`Access`]: TokenType::Access
    #[must_use]
    pub fn is_access(&self) -> bool {
        matches!(self, Self::Access)
    }

    /// Returns `true` if the token type is [`Refresh`].
    ///
    /// [`Refresh`]: TokenType::Refresh
    #[must_use]
    pub fn is_refresh(&self) -> bool {
        matches!(self, Self::Refresh)
    }

    /// Returns `true` if the token type is [`Signup`].
    ///
    /// [`Signup`]: TokenType::Signup
    #[must_use]
    pub fn is_signup(&self) -> bool {
        matches!(self, Self::Signup)
    }
}

#[derive(Debug, Serialize, Union)]
pub enum AuthResult {
    UserNotSignedUp(UserNotSignedUp),
    UserSignedUp(UserSignedUp),
}

impl AuthResult {
    pub fn as_user_signed_up(&self) -> Option<&UserSignedUp> {
        if let Self::UserSignedUp(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn try_into_user_signed_up(self) -> Result<UserSignedUp, Self> {
        if let Self::UserSignedUp(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

#[derive(Debug, Serialize, SimpleObject)]
pub struct UserNotSignedUp {
    pub signup_token: String,
}

#[derive(Debug, Serialize, SimpleObject)]
pub struct UserSignedUp {
    pub access_token: String,
    pub refresh_token: String,
}

pub fn create_tokens(
    user_id: Option<String>,
    email: Option<String>,
    phone_number: Option<String>,
) -> anyhow::Result<AuthResult> {
    let now = std::time::SystemTime::now();
    let exp = now.duration_since(std::time::UNIX_EPOCH)?.as_secs() as usize + (15 * 60);
    let access_secret = std::env::var("ACCESS_JWT_SECRET")?;

    if user_id.is_none() {
        let claims = Claims {
            phone_number: phone_number.clone(),
            email: email.clone(),
            user_id: user_id.clone(),
            exp,
            token_type: TokenType::Signup,
        };
        let access_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &EncodingKey::from_secret(access_secret.as_bytes()),
        )?;
        Ok(AuthResult::UserNotSignedUp(UserNotSignedUp {
            signup_token: access_token,
        }))
    } else {
        let claims = Claims {
            phone_number: phone_number.clone(),
            email: email.clone(),
            user_id: user_id.clone(),
            exp,
            token_type: TokenType::Access,
        };
        let access_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &EncodingKey::from_secret(access_secret.as_bytes()),
        )?;

        let refresh_secret = std::env::var("REFRESH_JWT_SECRET")?;
        let exp =
            now.duration_since(std::time::UNIX_EPOCH)?.as_secs() as usize + (30 * 24 * 60 * 60);
        let claims = Claims {
            phone_number,
            email,
            user_id,
            exp,
            token_type: TokenType::Refresh,
        };
        let refresh_token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &EncodingKey::from_secret(refresh_secret.as_bytes()),
        )?;

        Ok(AuthResult::UserSignedUp(UserSignedUp {
            access_token,
            refresh_token,
        }))
    }
}

pub fn decode_access_token(token: &str) -> anyhow::Result<Claims> {
    let access_secret = std::env::var("ACCESS_JWT_SECRET")?;
    let claims = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(access_secret.as_bytes()),
        &Validation::default(),
    )?;
    if claims.claims.token_type.is_access() || claims.claims.token_type.is_signup() {
        Ok(claims.claims)
    } else {
        Err(anyhow::anyhow!("Token is not valid ACCESS_TOKEN"))
    }
}

pub fn decode_refresh_token(token: &str) -> anyhow::Result<Claims> {
    let access_secret = std::env::var("REFRESH_JWT_SECRET")?;
    let claims = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(access_secret.as_bytes()),
        &Validation::default(),
    )?;
    if claims.claims.token_type.is_refresh() {
        Ok(claims.claims)
    } else {
        Err(anyhow::anyhow!("Token is not valid REFRESH_TOKEN"))
    }
}
