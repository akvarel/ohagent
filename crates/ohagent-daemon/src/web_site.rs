//! Same-origin static site. API misses never return a successful SPA document.
use axum::{
    http::{HeaderValue, Request},
    middleware::{self, Next},
    response::Response,
    routing::get_service,
    Router,
};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

pub fn attach(app: Router) -> anyhow::Result<Router> {
    let Some(directory) = std::env::var_os("OHAGENT_DASHBOARD_DIR") else {
        return Ok(app);
    };
    let directory = PathBuf::from(directory);
    anyhow::ensure!(
        directory.join("index.html").is_file(),
        "OHAGENT_DASHBOARD_DIR must contain a built index.html"
    );
    let index = directory.join("index.html");
    let mut app = app;
    for path in [
        "/",
        "/agent-as-a-service",
        "/agent-as-a-service/demo",
        "/agent-as-a-service/app",
        "/agent-as-a-service/app/{*path}",
        "/operator",
        "/operator/{*path}",
        "/progress",
        "/skills",
        "/skills/{id}",
        "/memory",
        "/keys",
        "/usage",
    ] {
        app = app.route_service(path, get_service(ServeFile::new(&index)));
    }
    Ok(app
        .fallback_service(ServeDir::new(directory).append_index_html_on_directories(false))
        .layer(middleware::from_fn(headers)))
}

async fn headers(request: Request<axum::body::Body>, next: Next) -> Response {
    let path = request.uri().path();
    let private = path.starts_with("/api/")
        || path.starts_with("/operator")
        || path.starts_with("/agent-as-a-service/app")
        || path == "/agent-as-a-service/demo"
        || ["/progress", "/skills", "/memory", "/keys", "/usage"]
            .iter()
            .any(|p| path.starts_with(p));
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if private {
        headers.insert(
            "x-robots-tag",
            HeaderValue::from_static("noindex, nofollow, noarchive"),
        );
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
    }
    response
}
