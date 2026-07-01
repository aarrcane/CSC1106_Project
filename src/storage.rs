use reqwest::Client;

#[derive(Clone)]
pub struct SupabaseStorage {
    pub client: Client,
    pub base_url: String,
    pub bucket: String,
    pub service_role_key: String,
}

impl SupabaseStorage {
    // Build the storage client from the current environment configuration.
    pub fn from_env() -> Self {
        let base_url = std::env::var("SUPABASE_URL").unwrap_or_else(|_| {
            eprintln!("SUPABASE_URL not found");
            String::new()
        });
        let bucket = std::env::var("SUPABASE_BUCKET").unwrap_or_else(|_| {
            eprintln!("SUPABASE_BUCKET not found");
            String::new()
        });
        let service_role_key = std::env::var("SUPABASE_SERVICE_ROLE_KEY").unwrap_or_else(|_| {
            eprintln!("SUPABASE_SERVICE_ROLE_KEY not found");
            String::new()
        });

        Self {
            client: reqwest::Client::new(),
            base_url: normalize_base_url(&base_url),
            bucket: bucket.trim_matches('/').to_string(),
            service_role_key,
        }
    }

    /// Upload raw bytes to Supabase Storage. Returns the object path on success.
    pub async fn upload(
        &self,
        object_path: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), String> {
        let url = format!(
            "{}/storage/v1/object/{}/{}",
            self.base_url, self.bucket, object_path
        );

        let res = self
            .client
            .post(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Content-Type", content_type)
            .header("x-upsert", "true") // overwrite if same name
            .body(bytes)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if res.status().is_success() {
            Ok(())
        } else {
            let text = res.text().await.unwrap_or_default();
            Err(format!("Supabase upload failed: {text}"))
        }
    }

    /// Delete an object from Supabase Storage.
    pub async fn delete(&self, object_path: &str) -> Result<(), String> {
        let url = format!(
            "{}/storage/v1/object/{}/{}",
            self.base_url, self.bucket, object_path
        );

        let res = self
            .client
            .delete(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if res.status().is_success() {
            Ok(())
        } else {
            let text = res.text().await.unwrap_or_default();
            Err(format!("Supabase delete failed: {text}"))
        }
    }

    /// Returns the public URL for an object (bucket must be public).
    pub fn public_url(&self, object_path: &str) -> String {
        format!(
            "{}/storage/v1/object/public/{}/{}",
            self.base_url, self.bucket, object_path
        )
    }
}

fn normalize_base_url(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/rest/v1")
        .trim_end_matches('/')
        .to_string()
}
