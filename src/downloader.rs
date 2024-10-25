use reqwest::header::HeaderMap;
use url::Url;

// todo: reuse the client
pub async fn download_file(_client: &reqwest::Client, url: &Url, headers: Option<HeaderMap>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    println!("[debug] Downloading file: {}", url);
    if let Some(hdrs) = &headers {
        for (name, value) in hdrs.iter() {
            if let Ok(value_str) = value.to_str() {
                println!("[debug] Header: {}: {}", name, value_str);
            } else {
                println!("[debug] Header: {}: <binary>", name);
            }
        }
    }

    let client = reqwest::Client::new();
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