use reqwest::header::HeaderMap;
use url::Url;

// todo: reuse the client
pub async fn download_file(client: &reqwest::Client, url: &Url, headers: Option<HeaderMap>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    println!("[debug] Downloading file: {}", url);

    let request = client.get(url.as_str());

    let request = if let Some(hdrs) = headers {
        request.headers(hdrs)
    } else {
        request
    };

    let response = request.send().await?;

    if response.status().is_success() {
        let content = response.bytes().await?.to_vec();
        Ok(content)
    } else {
        Err(format!("Failed to download file: {}", response.status()).into())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use super::*;

    #[tokio::test]
    async fn test_file_download() {
        let client = reqwest::Client::new();
        let url = Url::parse("https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/bipbop_4x3_variant.m3u8").unwrap();
        let headers = None;

        let content = download_file(&client, &url, headers).await.unwrap();
        assert!(content.len() > 0);
    }
}