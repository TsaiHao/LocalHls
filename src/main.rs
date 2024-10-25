mod downloader;
mod server;
mod utils;

use url::Url;
use reqwest;
use tokio;
use m3u8_rs;
use m3u8_rs::Playlist;
use m3u8_rs::MasterPlaylist;
use futures::stream::{StreamExt, FuturesOrdered};
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

async fn handle_media_manifest(manifest_url: &Url, base_url: &Url, config: &StreamConfig) -> Result<(), Box<dyn std::error::Error>> {
    println!("-------------------------");
    println!("Processing media playlist: {}", config.url);

    let mut path = utils::get_relative_path(base_url, manifest_url)?;
    let media_file_path = config.output_dir.join(&path);

    let content = downloader::download_file(&config.client, manifest_url, None).await?;
    utils::save_file(&content, &media_file_path)?;
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
    let base_url = utils::get_base_url(&manifest_url);

    for (i, segment) in manifest.segments.iter().enumerate() {
        let segment_uri = base_url.join(&segment.uri)?;
        let segment_file_path = output_dir.join(&segment.uri);
        let short_uri = segment.uri.clone();

        segment_tasks.push_back(async move {
            println!("[{}/{}]Start processing segment: {}", i + 1, segment_count, short_uri);
            let segment_content = downloader::download_file(&config.client, &segment_uri, None).await?;
            utils::save_file(&segment_content, &segment_file_path)?;
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
    let base_url = utils::get_base_url(&config.url);

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

    utils::create_dir_if_not_exists(&stream_config.output_dir)?;

    let manifest = downloader::download_file(&stream_config.client, &stream_config.url, None).await?;
    
    match m3u8_rs::parse_playlist(&manifest) {
        Result::Ok((_, Playlist::MasterPlaylist(playlist))) => {
            println!("Master playlist found");
            let master_file_name = utils::get_filename_from_url(&stream_config.url)
                .ok_or("Failed to get filename from URL")?;
            let master_file_path = stream_config.output_dir.join(master_file_name);

            utils::save_file(&manifest, &master_file_path)?;
            println!("Master playlist saved to: {}", master_file_path.display());

            handle_master_manifest(playlist, &stream_config).await?;
        },
        Result::Ok((_, Playlist::MediaPlaylist(_))) => {
            println!("Media playlist found");
            let base_url = utils::get_base_url(&stream_config.url);
            handle_media_manifest(&stream_config.url, &base_url, &stream_config).await?;
        },
        Result::Err(e) => {
            println!("Error: {:?}", e);
            Err("Failed to parse m3u8 playlist")?
        }
    }

    server::serve_files(server_port, &stream_config.output_dir).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
