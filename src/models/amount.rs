use async_graphql::SimpleObject;

#[derive(SimpleObject, Clone)]
pub struct Amount {
    pub amount: i64,
    pub currency_id: String,
}
