use warp::Filter;
use crate::StreamConfig;

pub async fn serve_files(stream_config: &StreamConfig) -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = &stream_config.output_dir;
    let port = stream_config.port;

    let html_code = format!("Root directory: {}", root_dir.display());
    let end = warp::path::end().map(move || {
        warp::reply::html(html_code.clone())
    });

    let file_server = warp::fs::dir(root_dir.clone());
    let routes = end.or(file_server);

    println!("=============================");
    println!("Starting server on port: {}", port);
    println!("Press Ctrl+C to stop the server");

    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
    Ok(())
}