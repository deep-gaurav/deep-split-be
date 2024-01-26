use std::{sync::Arc, time::Duration};

use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    EmptySubscription, Schema,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Extension, Router, Server,
};
use axum_auth::AuthBearer;
use expire_map::ExpiringHashMap;
use http_cache::{CACacheManager, CacheMode, HttpCache};
use http_cache_reqwest::Cache;

use models::currency::Currency;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use schema::{
    mutation::{Mutation, OtpMap},
    query::Query,
};

use sqlx::SqlitePool;
use tower_http::{compression::CompressionLayer, cors::CorsLayer};

use crate::{
    auth::{decode_access_token, AuthTypes, ForwardedHeader},
    models::user::User,
};

use serde::{Deserialize, Serialize};

pub mod auth;
pub mod email;
pub mod expire_map;
pub mod models;
pub mod notification;
pub mod schema;

type MainSchema = Schema<Query, Mutation, EmptySubscription>;

static REQWEST_CLIENT: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(Client::new())
        .with(Cache(HttpCache {
            mode: CacheMode::Default,
            manager: CACacheManager::default(),
            options: None,
        }))
        .build()
});

static FIREBASE_VALUES: Lazy<FirebaseValues> = Lazy::new(|| {
    let service_json_file = std::env::var("SERVICE_JSON").expect("No SERVICE_JSON defined");
    let data = std::fs::read_to_string(&service_json_file).unwrap();
    let data = serde_json::from_str(&data).unwrap();
    data
});

#[derive(Serialize, Deserialize)]
pub struct FirebaseValues {
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let _ = dotenvy::dotenv();
    pretty_env_logger::init();

    let asn_filepath = std::env::var("GEO_ASN_COUNTRY_CSV").expect("GEO_ASN_COUNTRY_CSV not var");
    let asn_db = ip2country::AsnDB::default()
        .load_ipv4(&asn_filepath)
        .expect("INVALID ASN");

    let pool = SqlitePool::connect(
        &std::env::var("DATABASE_URL").expect("NO DATABASE_URL in environment"),
    )
    .await
    .expect("Cannot connect to pool");

    let otp_map: OtpMap = OtpMap::new(ExpiringHashMap::new(Duration::from_secs(5 * 60)));

    let schema = MainSchema::build(Query, Mutation, EmptySubscription)
        .data(otp_map)
        .data(asn_db)
        .extension(async_graphql::extensions::ApolloTracing)
        .finish();

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        // allow requests from any origin
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any);
    let app = Router::new()
        .route("/playground", get(graphql_playground))
        .route("/", post(graphql_handler))
        // .route("/*path", get(files_handler))
        .with_state(pool.clone())
        .layer(Extension(schema))
        .layer(cors)
        .layer(CompressionLayer::new());

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".into());

    let mut currency_update_interval =
        tokio::time::interval(std::time::Duration::from_secs(60 * 60 * 12));
    tokio::spawn(async move {
        currency_update_interval.tick().await;
        let _ = Currency::fill_currencies(&pool).await;
    });
    Server::bind(&format!("0.0.0.0:{port}").parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn graphql_handler(
    Extension(schema): Extension<MainSchema>,

    token: Option<AuthBearer>,
    State(pool): State<SqlitePool>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> Result<GraphQLResponse, (StatusCode, String)> {
    let mut req = req.into_inner();
    let auth_type = 'auth_type: {
        if let Some(AuthBearer(token)) = token {
            let claims = decode_access_token(&token);
            if let Ok(claims) = claims {
                if claims.token_type.is_signup() {
                    break 'auth_type AuthTypes::AuthorizedNotSignedUp(claims);
                } else if claims.token_type.is_access() && claims.user_id.is_some() {
                    let user = User::get_from_id(&claims.user_id.unwrap(), &pool).await;
                    if let Ok(user) = user {
                        break 'auth_type AuthTypes::AuthorizedUser(user);
                    }
                }
            }

            AuthTypes::UnAuthorized
        } else {
            log::debug!("no token found");
            AuthTypes::UnAuthorized
        }
    };

    log::debug!("Setting authType {auth_type:#?}");
    req = req.data(auth_type);
    req = req.data(pool);
    if let Some(forwarded) = headers.get("X-Forwarded-For").and_then(|f| f.to_str().ok()) {
        req = req.data(ForwardedHeader(forwarded.to_string()));
    }

    // headers.get("Auth")
    Ok(schema.execute(req).await.into())
}

async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/")))
}
