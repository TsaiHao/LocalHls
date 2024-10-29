mod downloader;
mod server;
mod utils;

use futures::stream::{FuturesOrdered, StreamExt};
use m3u8_rs;
use m3u8_rs::MasterPlaylist;
use m3u8_rs::Playlist;
use reqwest;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use tokio;
use toml;
use url::Url;

#[derive(Deserialize)]
struct Args {
    /// The URL of the m3u8 playlist
    url: String,
    /// The output directory
    output: String,
    /// Additional headers to send with the request
    headers: Option<HashMap<String, HeadersValue>>,
    /// The number of segments to fetch
    duration: Option<f32>,
    /// The number of segments to fetch
    count: Option<usize>,
    /// Server port to use
    port: Option<u16>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum HeadersValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Deserialize, Debug)]
enum FetchLength {
    Duration(f32),
    Count(usize),
}

struct StreamConfig {
    client: reqwest::Client,
    url: Url,
    headers: Option<HeaderMap>,
    output_dir: std::path::PathBuf,
    length: FetchLength,
    port: u16,
}

fn parse_headers(headers: &HashMap<String, HeadersValue>) -> Option<HeaderMap> {
    let mut header_map = HeaderMap::new();
    for (name, value) in headers {
        match value {
            HeadersValue::Single(value) => {
                header_map.insert(
                    HeaderName::from_str(name).ok()?,
                    HeaderValue::from_str(value).ok()?,
                );
            }
            HeadersValue::Multiple(values) => {
                for value in values.iter() {
                    header_map.append(
                        HeaderName::from_str(name).ok()?,
                        HeaderValue::from_str(value).ok()?,
                    );
                }
            }
        }
    }
    Some(header_map)
}

async fn handle_media_manifest(
    manifest_url: &Url,
    base_url: &Url,
    config: &StreamConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("-------------------------");
    println!("Processing media playlist: {}", config.url);

    let mut path = utils::get_relative_path(base_url, manifest_url)?;
    let media_file_path = config.output_dir.join(&path);

    let content =
        downloader::download_file(&config.client, manifest_url, config.headers.clone()).await?;
    utils::save_file(&content, &media_file_path)?;
    println!("Media playlist saved to: {}", media_file_path.display());

    let manifest = match m3u8_rs::parse_playlist(&content) {
        Ok((_, Playlist::MediaPlaylist(pl))) => pl,
        Ok((_, Playlist::MasterPlaylist(_))) => {
            return Err("Trying to process master playlist as media list".into())
        }
        Err(_) => return Err("Not a media playlist".into()),
    };

    let mut output_dir = config.output_dir.clone();
    if path.pop() {
        output_dir = output_dir.join(&path);
    }
    let mut segment_count = manifest.segments.len();
    match config.length {
        FetchLength::Duration(duration) => {
            let mut dur_sum = 0.0;
            for (i, segment) in manifest.segments.iter().enumerate() {
                dur_sum += segment.duration;
                if dur_sum >= duration {
                    segment_count = i;
                    println!("Duration limit reached at segment: {}", i);
                    break;
                }
            }
        },
        FetchLength::Count(count) => {
            segment_count = std::cmp::min(segment_count, count);
        }
    }

    let mut segment_tasks = FuturesOrdered::new();
    let base_url = utils::get_base_url(&manifest_url);

    for (i, segment) in manifest.segments.iter().enumerate() {
        if i >= segment_count {
            break;
        }
        let segment_uri = base_url.join(&segment.uri)?;
        let segment_file_path = output_dir.join(&segment.uri);
        if segment_file_path.exists() {
            println!(
                "[{}/{}]Segment already exists: {}",
                i + 1,
                segment_count,
                segment_file_path.display()
            );
            continue;
        }
        let short_uri = segment.uri.clone();

        segment_tasks.push_back(async move {
            println!(
                "[{}/{}]Start processing segment: {}",
                i + 1,
                segment_count,
                short_uri
            );
            let segment_content =
                downloader::download_file(&config.client, &segment_uri, config.headers.clone())
                    .await?;
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

async fn handle_master_manifest(
    playlist: MasterPlaylist,
    config: &StreamConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = utils::get_base_url(&config.url);

    let variant_count = playlist.variants.len();
    println!("Processing {} variants", variant_count);
    for (i, variant) in playlist.variants.iter().enumerate() {
        println!(
            "[{}/{}] Processing variant: {}",
            i + 1,
            variant_count,
            variant.uri
        );
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

    let output_dir = std::path::absolute(&args.output)?;

    let client = reqwest::Client::new();
    let port = args.port.unwrap_or(3030);
    let headers = match &args.headers {
        Some(headers) => parse_headers(headers),
        None => None,
    };
    let length = if let Some(duration) = args.duration {
        FetchLength::Duration(duration)
    } else if let Some(count) = args.count {
        FetchLength::Count(count)
    } else {
        FetchLength::Count(usize::MAX)
    };

    let stream_config = StreamConfig {
        client,
        output_dir,
        url: Url::parse(args.url.as_str())?,
        headers,
        length,
        port,
    };

    println!("-------------------------");
    println!("Received Parameters:");
    println!("URL: {}", stream_config.url);
    println!("Output Directory: {}", stream_config.output_dir.display());
    if let Some(headers) = &args.headers {
        println!("Headers: {:?}", headers);
    }
    println!("-------------------------");

    utils::create_dir_if_not_exists(&stream_config.output_dir)?;

    let manifest = downloader::download_file(
        &stream_config.client,
        &stream_config.url,
        stream_config.headers.clone(),
    )
    .await?;

    match m3u8_rs::parse_playlist(&manifest) {
        Ok((_, Playlist::MasterPlaylist(playlist))) => {
            println!("Master playlist found");
            let master_file_name = utils::get_filename_from_url(&stream_config.url)
                .ok_or("Failed to get filename from URL")?;
            let master_file_path = stream_config.output_dir.join(master_file_name);

            utils::save_file(&manifest, &master_file_path)?;
            println!("Master playlist saved to: {}", master_file_path.display());

            handle_master_manifest(playlist, &stream_config).await?;
        }
        Ok((_, Playlist::MediaPlaylist(_))) => {
            println!("Media playlist found");
            let base_url = utils::get_base_url(&stream_config.url);
            handle_media_manifest(&stream_config.url, &base_url, &stream_config).await?;
        }
        Err(e) => {
            println!("Error: {:?}", e);
            Err("Failed to parse m3u8 playlist")?
        }
    }

    server::serve_files(&stream_config).await?;

    Ok(())
}
