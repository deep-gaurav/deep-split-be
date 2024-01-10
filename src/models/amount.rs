use async_graphql::SimpleObject;

#[derive(SimpleObject)]
pub struct Amount {
    pub amount: i64,
    pub currency_id: String,
}
