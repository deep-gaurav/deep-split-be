use axum::http::{HeaderMap, HeaderValue};
use s3::{creds::Credentials, Bucket};
// use aws_config::meta::region::RegionProviderChain;
// use aws_sdk_s3::{config::Credentials, presigning::PresigningConfig};
use uuid::Uuid;

pub struct S3 {
    // r2_access_key_id: String,
    // r2_secret_access_key: String,
    // r2_endpoint_url: String,
    // r2_bucket: String,
    // s3_client: aws_sdk_s3::Client,
    public_url: String,
    bucket: Bucket,
}

impl S3 {
    pub async fn init_from_env() -> anyhow::Result<Self> {
        let access_key_id = std::env::var("R2_ACCESS_KEY_ID").expect("no var R2_ACCESS_KEY_ID");
        let secret_access_key =
            std::env::var("R2_SECRET_ACCESS_KEY").expect("no var R2_SECRET_ACCESS_KEY");
        let r2_account_id = std::env::var("R2_ACCOUNT_ID").expect("no var R2_ACCOUNT_ID");
        let r2_bucket = std::env::var("R2_BUCKET").expect("no var R2_BUCKET");
        let public_url =
            std::env::var("R2_BUCKET_PUBLIC_URL").expect("no var R2_BUCKET_PUBLIC_URL");

        let credentials = Credentials::new(
            Some(&access_key_id),
            Some(&secret_access_key),
            None,
            None,
            None,
        )?;
        let bucket = s3::Bucket::new(
            &r2_bucket,
            s3::Region::R2 {
                account_id: r2_account_id,
            },
            credentials,
        )?;

        Ok(Self { bucket, public_url })
    }

    pub async fn new_image_upload_presign_url(
        &self,
        id: &Uuid,
        file_size: u64,
    ) -> anyhow::Result<String> {
        let mut headers = HeaderMap::new();
        headers.append("content-length", file_size.into());
        headers.append("content-type", HeaderValue::from_static("image/avif"));
        let url = self
            .bucket
            .presign_put(format!("fe_image/{id}.avif"), 15 * 60, Some(headers))?;
        Ok(url)
    }

    pub async fn move_to_be(&self, id: &str) -> anyhow::Result<()> {
        log::info!("Moving fe_image/{id}.avif to images/{id}.avif");
        let status = self
            .bucket
            .copy_object_internal(format!("fe_image/{id}.avif"), format!("images/{id}.avif"))
            .await?;
        log::info!("Moving status: {status}");
        Ok(())
    }

    pub fn get_public_url(&self, id: &str) -> String {
        format!("{}/images/{id}.avif", self.public_url)
    }
}
