use std::time::Duration;

use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    EmptyMutation, EmptySubscription, Schema,
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
use jsonwebtoken::DecodingKey;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use schema::{
    mutation::{Mutation, OtpMap},
    query::Query,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;

use crate::{
    auth::{decode_access_token, AuthTypes, Claims},
    models::user::User,
};

pub mod auth;
pub mod email;
pub mod expire_map;
pub mod models;
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

#[tokio::main]
async fn main() -> Result<(), ()> {
    dotenvy::dotenv().expect("No env");
    pretty_env_logger::init();

    let pool = SqlitePool::connect(
        &std::env::var("DATABASE_URL").expect("NO DATABASE_URL in environment"),
    )
    .await
    .expect("Cannot connect to pool");

    let otp_map: OtpMap = OtpMap::new(ExpiringHashMap::new(Duration::from_secs(15 * 60)));

    let schema = MainSchema::build(Query, Mutation, EmptySubscription)
        .data(otp_map)
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
        .with_state(pool)
        .layer(Extension(schema))
        .layer(cors);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".into());
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

    // headers.get("Auth")
    Ok(schema.execute(req).await.into())
}

async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/")))
}
