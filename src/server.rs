use warp::Filter;

pub async fn serve_files(port: u16, root_dir: &std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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