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
use http_cache::{CACacheManager, CacheMode, HttpCache};
use http_cache_reqwest::Cache;
use jsonwebtoken::DecodingKey;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use schema::{mutation::Mutation, query::Query};
use serde::Deserialize;
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;

use crate::{auth::AuthTypes, models::user::User};

pub mod auth;
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

    let schema = MainSchema::build(Query, Mutation, EmptySubscription)
        .extension(async_graphql::extensions::ApolloTracing)
        .finish();

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        // allow requests from any origin
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
    headers: HeaderMap,
    req: GraphQLRequest,
) -> Result<GraphQLResponse, (StatusCode, String)> {
    let mut req = req.into_inner();
    let auth_type = 'auth_type: {
        if let Some(AuthBearer(token)) = token {
            let header = jsonwebtoken::decode_header(&token);
            if let Ok(header) = header {
                // log::info!("header {:#?}", header);
                if let Some(kid) = &header.kid {
                    let resp =  REQWEST_CLIENT.get("https://www.googleapis.com/robot/v1/metadata/x509/securetoken@system.gserviceaccount.com").send().await;
                    if let Ok(resp) = resp {
                        let json = resp.json::<serde_json::Value>().await;
                        if let Ok(json) = json {
                            let sign = json
                                .get(kid)
                                .and_then(|val| val.as_str())
                                .and_then(|val| DecodingKey::from_rsa_pem(val.as_bytes()).ok());
                            if let Some(sign) = sign {
                                #[derive(Debug, Deserialize)]
                                struct Claims {
                                    // aud: String,         // Optional. Audience
                                    // exp: usize,          // Required (validate_exp defaults to true in validation). Expiration time (as UTC timestamp)
                                    // iat: usize,          // Optional. Issued at (as UTC timestamp)
                                    // sub: String,         // Optional. Subject (whom token refers to)
                                    phone_number: String,
                                }
                                // log::info!("key {:#?}",);
                                let claims = jsonwebtoken::decode::<Claims>(
                                    &token,
                                    &sign,
                                    &jsonwebtoken::Validation::new(header.alg),
                                );
                                match claims {
                                    Ok(claims) => {
                                        let user = User::get_from_phone(
                                            &claims.claims.phone_number,
                                            &pool,
                                        )
                                        .await;
                                        if let Ok(user) = user {
                                            break 'auth_type AuthTypes::AuthorizedUser(user);
                                        } else {
                                            break 'auth_type AuthTypes::AuthorizedNotSignedUp(
                                                claims.claims.phone_number,
                                            );
                                        }
                                    }
                                    Err(err) => {
                                        log::warn!("jwt error {:#?}", err)
                                    }
                                }
                            }
                        }
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
