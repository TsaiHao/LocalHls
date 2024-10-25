use std::io::Write;
use url::Url;

pub fn get_filename_from_url(url: &Url) -> Option<String> {
    if url.path().ends_with('/') {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<&str>>();
    let mut name = segments[segments.len() - 1].to_string();
    if name.starts_with('.') && name.len() > 0 {
        name = format!("{}{}", segments[segments.len() - 1], name);
    }
    Some(name)
}

pub fn create_dir_if_not_exists(dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

pub fn save_file(content: &Vec<u8>, output_file: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let dir_name = output_file.parent()
        .expect(format!("Failed to get parent directory for: {}", output_file.display()).as_str());
    create_dir_if_not_exists(dir_name)?;

    let mut file = std::fs::File::create(output_file)?;
    file.write_all(&content)?;
    Ok(())
}

pub fn get_relative_path(base: &Url, target: &Url) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
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

pub fn get_base_url(url: &Url) -> Url {
    let base_url = url.clone();
    base_url.join("./").unwrap()
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
}