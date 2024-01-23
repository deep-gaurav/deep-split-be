use serde::{Serialize,Deserialize};
use std::{
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{REQWEST_CLIENT, FirebaseValues, FIREBASE_VALUES};

static GRANT_TYPE: &'static str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

pub async fn get_bearer_token() -> Result<String, anyhow::Error> {
    #[derive(Serialize, Deserialize)]
    struct Claims {
        iss: String,
        scope: String,
        aud: String,
        exp: i64,
        iat: i64,
    }

    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let now_secs = since_the_epoch.as_secs();
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(&FIREBASE_VALUES.private_key.as_bytes())?;
    let claims = Claims {
            iss: FIREBASE_VALUES.client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/firebase.database https://www.googleapis.com/auth/firebase.messaging https://www.googleapis.com/auth/identitytoolkit https://www.googleapis.com/auth/userinfo.email".into(),
            aud: "https://accounts.google.com/o/oauth2/token".to_string(),
            exp: (now_secs + 3600) as i64,
            iat: now_secs as i64,
        };
    let jwt = jsonwebtoken::encode(&header, &claims, &key)?;

    #[derive(Serialize, Deserialize)]
    struct ReqBody {
        grant_type: String,
        assertion: String,
    }

    #[derive(Serialize, Deserialize)]
    struct ResBody {
        access_token: String,
        token_type: String,
        expires_in: u64,
    }
    let body = serde_urlencoded::to_string(ReqBody {
        grant_type: GRANT_TYPE.to_string(),
        assertion: jwt,
    })?;
    let data = REQWEST_CLIENT
        .post("https://accounts.google.com/o/oauth2/token")
        .body(body)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await?
        .json::<ResBody>()
        .await?;
    Ok(data.access_token)
}

pub async fn send_message_notification_with_retry(
    title: &str,

    path_url: &str,
    full_url: &str,
    description: &str,
    token: &str,
    bearer: Arc<RwLock<Option<String>>>,
) -> Result<(), anyhow::Error> {
    log::info!("Sending notificaion try 1");
    let result = send_message_notification(
        title,
        path_url,
        full_url,
        description,
        token,
        bearer.clone(),
        true,
    )
    .await;
    log::info!("Sending notificaion try 1 result {result:#?}");

    match result {
        Ok(_) => Ok(()),
        Err(_) => {
            send_message_notification(title, path_url, full_url, description, token, bearer, false)
                .await
        }
    }
}

pub async fn send_message_notification(
    title: &str,
    path_url: &str,
    full_url: &str,
    description: &str,
    token: &str,
    bearer: Arc<RwLock<Option<String>>>,
    retry: bool,
) -> Result<(), anyhow::Error> {
    let bearer_token = {
        if retry {
            let old_token = bearer.read().unwrap().clone();
            match old_token {
                Some(old_token) => old_token,
                None => {
                    let new_token = get_bearer_token().await?;
                    *bearer.write().unwrap() = Some(new_token.clone());
                    new_token
                }
            }
        } else {
            let new_token = get_bearer_token().await?;
            *bearer.write().unwrap() = Some(new_token.clone());
            new_token
        }
    };
    #[derive(Serialize)]
    struct Notification {
        title: String,
        body: String,
    }

    #[derive(Serialize)]
    struct NotificationData {
        url: String,
    }
    #[derive(Serialize)]
    struct Message {
        notification: Notification,
        token: String,
        data: NotificationData,
        webpush: WebPush,
    }

    #[derive(Serialize)]
    struct WebPush {
        fcm_options: WebPushFcmOptions,
    }

    #[derive(Serialize)]
    struct WebPushFcmOptions {
        link: String,
    }

    #[derive(Serialize)]
    struct Body {
        message: Message,
    }

    let body = Body {
        message: Message {
            notification: Notification {
                title: title.to_string(),
                body: description.to_string(),
            },
            token: token.to_string(),
            data: NotificationData {
                url: path_url.to_string(),
            },
            webpush: WebPush {
                fcm_options: WebPushFcmOptions {
                    link: full_url.to_string(),
                },
            },
        },
    };

    let body_string = serde_json::to_string(&body).unwrap();
    let response = REQWEST_CLIENT
        .post("https://fcm.googleapis.com/v1/projects/secret-fire/messages:send")
        .header("Authorization", format!("Bearer {bearer_token}"))
        .body(body_string)
        .send()
        .await?;
    if response.status().is_success() {
        let _response = response.text().await?;
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await;
        log::warn!("Notification send error code {:#?} {:#?}", status, body);
        Err(anyhow::anyhow!(
            "Notification send error code {:#?} {:#?}",
            status,
            body
        ))
    }
}
