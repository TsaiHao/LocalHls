use reqwest;
use reqwest::header::HeaderMap;
use tokio;
use m3u8_rs;
use m3u8_rs::Playlist;

async fn download_file(url: &str, headers: Option<HeaderMap>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let request = client.get(url);
    
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

async fn handle_media_manifest(playlist: m3u8_rs::MediaPlaylist) {
}

async fn handle_master_manifest(playlist: m3u8_rs::MasterPlaylist) {
    for variant in playlist.variants {
        let mut name: String = "".to_owned();
        if let Some(v) = variant.video {
            name.push_str("video"); 
        }
        if let Some(a) = variant.audio {
            if name.len() > 0 {
                name.push_str(", ");
            }
            name.push_str("audio");
        }
        if let Some(s) = variant.subtitles {
            if name.len() > 0 {
                name.push_str(", ");
            }
            name.push_str("subtitles");
        }

        println!("Processing variant:");
        println!("uri: {}", variant.uri);
        println!("name: {}", name);
        println!("bandwidth: {}", variant.bandwidth);
        if let Some(res) = variant.resolution {
            println!("resolution: {}x{}", res.width, res.height);
        }
        if let Some(codecs) = variant.codecs {
            println!("codecs: {}", codecs);
        }
        println!("");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let m3u8_url = "https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/.m3u8";
    let output_dir = std::env::current_dir().unwrap();

    let manifest = download_file(m3u8_url, None).await?;
    
    match m3u8_rs::parse_playlist(&manifest) {
        Result::Ok((i, Playlist::MasterPlaylist(pl))) => {
            println!("Master playlist found");
            handle_master_manifest(pl).await;
        },
        Result::Ok((i, Playlist::MediaPlaylist(pl))) => {
            println!("Media playlist found");
            handle_media_manifest(pl).await;
        },
        Result::Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_download_fille() {
        let url = "https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/.m3u8";
        let headers = reqwest::header::HeaderMap::new();
        let content = download_file(url, Some(headers)).await.unwrap();
        let content_text = String::from_utf8(content).unwrap();
        assert!(content_text.contains("#EXTM3U"));
    }
}
