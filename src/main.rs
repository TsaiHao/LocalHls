use std::io::Write;
use url::Url;
use reqwest;
use reqwest::header::HeaderMap;
use tokio;
use m3u8_rs;
use m3u8_rs::Playlist;

async fn download_file(url: &Url, headers: Option<HeaderMap>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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

fn get_filename_from_url(url: &Url) -> String {
    let url_parts: Vec<&str> = url.path_segments().unwrap().collect();
    let mut name = url_parts[url_parts.len() - 1].to_owned();
    if name.contains("?") {
        let name_parts: Vec<&str> = name.split("?").collect();
        name = name_parts[0].to_owned();
    }
    if name.starts_with('.') {
        name = format!("{}{}", url_parts[url_parts.len() - 2], name);
    }
    name
}

fn create_dir_if_not_exists(dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn save_file(content: &Vec<u8>, output_file: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let dir_name = output_file.parent()
        .expect(format!("Failed to get parent directory for: {}", output_file.display()).as_str());
    create_dir_if_not_exists(dir_name)?;

    let mut file = std::fs::File::create(output_file)?;
    file.write_all(&content)?;
    Ok(())
}

fn get_relative_path(base: &Url, target: &Url) -> Result<String, Box<dyn std::error::Error>> {
    if base.host() != target.host() {
        return Err("Hosts are different".into());
    }
    let mut diff = "".to_string();
    let base_path_segments = base.path_segments().unwrap().collect::<Vec<&str>>();
    let target_path_segments = target.path_segments().unwrap().collect::<Vec<&str>>();
    let mut i = 0;
    while i < base_path_segments.len() && i < target_path_segments.len() {
        if base_path_segments[i] != target_path_segments[i] {
            break;
        }
        i += 1;
    }
    for seg in i..base_path_segments.len() {
        diff.push_str(target_path_segments[seg]);
    }
    Ok(diff)
}

fn get_base_url(url: &Url) -> Url {
    let base_url = url.clone();
    base_url.join("./").unwrap()
}

async fn handle_media_manifest(m3u8_url: &Url, base_url: &Url, output_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("-------------------------");
    println!("Processing media playlist: {}", m3u8_url);

    let path = get_relative_path(base_url, m3u8_url)?;
    let media_file_path = output_dir.join(&path);

    let content = download_file(&m3u8_url, None).await?;
    save_file(&content, &media_file_path)?;
    println!("Media playlist saved to: {}", media_file_path.display());

    let manifest = match m3u8_rs::parse_playlist(&content) {
        Ok((_, Playlist::MediaPlaylist(pl))) => pl,
        Ok((_, Playlist::MasterPlaylist(_))) => return Err("Trying to process master playlist as media list".into()),
        Err(_) => return Err("Not a media playlist".into())
    };

    let output_dir = output_dir.join(path.rsplitn(2, "/").last().unwrap());
    let segment_count = manifest.segments.len();

    for (i, segment) in manifest.segments.iter().enumerate() {
        println!("[{}/{}]Processing segment: {}", i, segment_count, segment.uri);

        let segment_uri = base_url.join(&segment.uri)?;
        let segment_content = download_file(&segment_uri, None).await?;
        let segment_file_path = output_dir.join(&segment.uri);

        save_file(&segment_content, &segment_file_path)?;

        println!("Segment saved to: {}", segment_file_path.display());
    }
    println!("--------------------------");
    Ok(())
}

async fn handle_master_manifest(m3u8_url: &Url, output_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let master_file_name = get_filename_from_url(m3u8_url);
    let master_file_path = output_dir.join(master_file_name);

    let content = download_file(m3u8_url, None).await?;
    save_file(&content, &master_file_path)?;
    println!("Master playlist saved to: {}", master_file_path.display());

    let master_list = match m3u8_rs::parse_playlist(&content) {
        Ok((_, Playlist::MasterPlaylist(pl))) => pl,
        Ok((_, Playlist::MediaPlaylist(_))) => return Err("Trying to process media playlist as master list".into()),
        Err(_) => return Err("Not a master playlist".into())
    };
    let base_url = get_base_url(m3u8_url);

    let variant_count = master_list.variants.len();
    for (i, variant) in master_list.variants.iter().enumerate() {
        println!("[{}/{}] Processing variant: {}", i, variant_count, variant.uri);
        let variant_url = base_url.join(&variant.uri)?;
        handle_media_manifest(&variant_url, &base_url, &output_dir).await?;
        println!("");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let m3u8_url = "https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/bipbop_4x3_variant.m3u8";
    let output_dir = std::env::current_dir().unwrap().join("output");
    create_dir_if_not_exists(&output_dir)?;

    let m3u8_url = Url::parse(m3u8_url)?;

    // Todo: remove redundant downloading of manifest
    let manifest = download_file(&m3u8_url, None).await?;
    
    match m3u8_rs::parse_playlist(&manifest) {
        Result::Ok((_, Playlist::MasterPlaylist(_))) => {
            println!("Master playlist found");
            handle_master_manifest(&m3u8_url, &output_dir).await?;
        },
        Result::Ok((_, Playlist::MediaPlaylist(_))) => {
            println!("Media playlist found");
            let base_url = get_base_url(&m3u8_url);
            handle_media_manifest(&m3u8_url, &base_url, &output_dir).await?;
        },
        Result::Err(e) => {
            println!("Error: {:?}", e);
            Err("Failed to parse m3u8 playlist")?
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_url_manipulation() {
        let test_url = Url::parse("https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/bipbop_4x3_variant.m3u8");
        assert!(test_url.is_ok());
        let test_url = test_url.unwrap();
        let base_url = get_base_url(&test_url);
        assert_eq!(base_url.as_str(), "https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/");
        let relative_path = get_relative_path(&base_url, &test_url);
        assert!(relative_path.is_ok());
        assert_eq!(relative_path.unwrap(), "bipbop_4x3_variant.m3u8");
    }
}
