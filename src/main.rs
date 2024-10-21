use std::io::Write;
use url::Url;
use reqwest;
use reqwest::header::HeaderMap;
use tokio;
use m3u8_rs;
use m3u8_rs::Playlist;
use m3u8_rs::MasterPlaylist;
use futures::stream::{StreamExt, FuturesOrdered};
use warp;
use warp::Filter;
use toml;
use serde::Deserialize;

#[derive(Deserialize)]
struct Args {
    /// The URL of the m3u8 playlist
    url: String,

    /// The output directory
    output: String,

    /// Additional headers to send with the request
    headers: Option<Vec<String>>,

    /// Server port to use
    port: Option<u16>,
}

struct StreamConfig {
    client: reqwest::Client,

    url: Url,

    output_dir: std::path::PathBuf,

    args: Args,
}

async fn download_file(client: &reqwest::Client, url: &Url, headers: Option<HeaderMap>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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

fn get_filename_from_url(url: &Url) -> Option<String> {
    if url.path().ends_with('/') {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<&str>>();
    let mut name = segments[segments.len() - 1].to_string();
    if name.starts_with('.') && name.len() > 1 {
        name = format!("{}{}", segments[segments.len() - 2], name);
    }
    Some(name)
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

fn get_relative_path(base: &Url, target: &Url) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // todo: url.make_relative?
    if base.host() != target.host() {
        return Err("Hosts are different".into());
    }
    let base_path_segments = base.path_segments().unwrap().collect::<Vec<&str>>();
    let target_path_segments = target.path_segments().unwrap().collect::<Vec<&str>>();
    let mut i = 0;
    while i < base_path_segments.len() && i < target_path_segments.len() {
        if base_path_segments[i] != target_path_segments[i] {
            break;
        }
        i += 1;
    }

    let mut rel_path = std::path::PathBuf::new();
    for seg in i..target_path_segments.len() {
        rel_path.push(target_path_segments[seg]);
    }
    Ok(rel_path)
}

fn get_base_url(url: &Url) -> Url {
    let base_url = url.clone();
    base_url.join("./").unwrap()
}

async fn handle_media_manifest(manifest_url: &Url, base_url: &Url, config: &StreamConfig) -> Result<(), Box<dyn std::error::Error>> {
    println!("-------------------------");
    println!("Processing media playlist: {}", config.url);

    let mut path = get_relative_path(base_url, manifest_url)?;
    let media_file_path = config.output_dir.join(&path);

    let content = download_file(&config.client, manifest_url, None).await?;
    save_file(&content, &media_file_path)?;
    println!("Media playlist saved to: {}", media_file_path.display());

    let manifest = match m3u8_rs::parse_playlist(&content) {
        Ok((_, Playlist::MediaPlaylist(pl))) => pl,
        Ok((_, Playlist::MasterPlaylist(_))) => return Err("Trying to process master playlist as media list".into()),
        Err(_) => return Err("Not a media playlist".into())
    };

    let mut output_dir = config.output_dir.clone();
    if path.pop() {
        output_dir = output_dir.join(&path);
    }
    let segment_count = manifest.segments.len();

    let mut segment_tasks = FuturesOrdered::new();
    let base_url = get_base_url(&config.url);

    for (i, segment) in manifest.segments.iter().enumerate() {
        let segment_uri = base_url.join(&segment.uri)?;
        let segment_file_path = output_dir.join(&segment.uri);
        let short_uri = segment.uri.clone();

        segment_tasks.push_back(async move {
            println!("[{}/{}]Start processing segment: {}", i + 1, segment_count, short_uri);
            let segment_content = download_file(&config.client, &segment_uri, None).await?;
            save_file(&segment_content, &segment_file_path)?;
            println!("Segment saved to: {}", segment_file_path.display());

            Ok::<(), Box<dyn std::error::Error>>(())
        });
    }

    while let Some(result) = segment_tasks.next().await {
        result?;
    }
    println!("--------------------------");
    Ok(())
}

async fn handle_master_manifest(playlist: MasterPlaylist, config: &StreamConfig) -> Result<(), Box<dyn std::error::Error>> {

    let base_url = get_base_url(&config.url);

    let variant_count = playlist.variants.len();
    println!("Processing {} variants", variant_count);
    for (i, variant) in playlist.variants.iter().enumerate() {
        println!("[{}/{}] Processing variant: {}", i + 1, variant_count, variant.uri);
        let variant_url = base_url.join(&variant.uri)?;
        handle_media_manifest(&variant_url, &base_url, &config).await?;
        println!("");
    }

    Ok(())
}

async fn serve_files(port: u16, root_dir: &std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = root_dir.to_path_buf();
    let html_code = format!("Root directory: {}", root_dir.display());
    let end = warp::path::end().map(move || {
        warp::reply::html(html_code.clone())
    });

    static mut ROOT_DIR: Option<std::path::PathBuf> = None;
    unsafe {
        ROOT_DIR = Some(root_dir.clone());
    }

    let file_server = warp::fs::dir(root_dir);
    let routes = end.or(file_server);

    println!("=============================");
    println!("Starting server on port: {}", port);
    println!("Press Ctrl+C to stop the server");

    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_file = std::env::args().nth(1).expect("No config file provided");
    let config_file = std::path::Path::new(&config_file);
    let config_text = std::fs::read_to_string(config_file)?;
    let args: Args = toml::from_str(&config_text)?;

    let client = reqwest::Client::new();
    let server_port = args.port.unwrap_or(3030);

    let stream_config = StreamConfig {
        client,
        output_dir: std::path::PathBuf::from(args.output.as_str()),
        url: Url::parse(args.url.as_str())?,
        args,
    };

    println!("-------------------------");
    println!("Received Parameters:");
    println!("URL: {}", stream_config.url);
    println!("Output Directory: {}", stream_config.output_dir.display());
    if let Some(headers) = &stream_config.args.headers {
        println!("Headers: {:?}", headers);
    }
    println!("-------------------------");

    create_dir_if_not_exists(&stream_config.output_dir)?;

    // Todo: remove redundant downloading of manifest
    let manifest = download_file(&stream_config.client, &stream_config.url, None).await?;
    
    match m3u8_rs::parse_playlist(&manifest) {
        Result::Ok((_, Playlist::MasterPlaylist(playlist))) => {
            println!("Master playlist found");
            let master_file_name = get_filename_from_url(&stream_config.url)
                .ok_or("Failed to get filename from URL")?;
            let master_file_path = stream_config.output_dir.join(master_file_name);

            save_file(&manifest, &master_file_path)?;
            println!("Master playlist saved to: {}", master_file_path.display());

            handle_master_manifest(playlist, &stream_config).await?;
        },
        Result::Ok((_, Playlist::MediaPlaylist(_))) => {
            println!("Media playlist found");
            let base_url = get_base_url(&stream_config.url);
            handle_media_manifest(&stream_config.url, &base_url, &stream_config).await?;
        },
        Result::Err(e) => {
            println!("Error: {:?}", e);
            Err("Failed to parse m3u8 playlist")?
        }
    }

    serve_files(server_port, &stream_config.output_dir).await?;

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
        let relative_path = get_relative_path(&base_url, &test_url).unwrap();
        assert_eq!(relative_path.to_str().unwrap(), "bipbop_4x3_variant.m3u8");

        let media_url = Url::parse("https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/gear1/prog_index.m3u8").unwrap();
        let relative_path = get_relative_path(&base_url, &media_url).unwrap();
        assert_eq!(relative_path.to_str().unwrap(), "gear1/prog_index.m3u8");
    }

    #[tokio::test]
    async fn test_get_filename_from_url() {
        let test_url = Url::parse("https://github.com/foo/bar/baz.txt?query=1&query=2").unwrap();

        let file_name = get_filename_from_url(&test_url).unwrap();
        assert_eq!(file_name, "baz.txt");

        let test_url = Url::parse("https://github.com/foo/bar/baz.txt").unwrap();
        let file_name = get_filename_from_url(&test_url).unwrap();
        assert_eq!(file_name, "baz.txt");

        let test_url = Url::parse("https://github.com/foo/bar/baz/").unwrap();
        let file_name = get_filename_from_url(&test_url);
        assert!(file_name.is_none());
    }

    #[tokio::test]
    async fn test_arg_parse() {
        let toml_text = r#"
url = "https://example.com"
output = "output"
headers = ["header1: value1", "header2: value2"]
port = 3030
        "#;
        let args: Args = toml::from_str(toml_text).unwrap();
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.output, "output");
        assert_eq!(args.headers.unwrap().len(), 2);
        assert_eq!(args.port.unwrap(), 3030);
    }
}
