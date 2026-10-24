use lookat::Result;
use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(arg) = env::args().nth(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "lookat-server\n提供 Lookat Chamber 界面与已鉴权数据接收，不采集服务器前台。\n配置：LOOKAT_PORT、LOOKAT_FRONTEND_DIR、LOOKAT_DATA_PATH。"
                );
                return Ok(());
            }
            _ => return Err(format!("未知参数 {arg}；使用 --help 查看用法").into()),
        }
    }
    let port = env::var("LOOKAT_PORT")
        .unwrap_or_else(|_| "53214".into())
        .parse::<u16>()?;
    let frontend =
        PathBuf::from(env::var_os("LOOKAT_FRONTEND_DIR").unwrap_or_else(|| "frontend/dist".into()));
    if !frontend.join("index.html").is_file() {
        return Err("缺少前端产物，请先执行 npm run build --prefix frontend".into());
    }
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
            .await?;
    let data = PathBuf::from(
        env::var_os("LOOKAT_DATA_PATH").unwrap_or_else(|| "data/lookat.sqlite3".into()),
    );
    let store = std::sync::Arc::new(std::sync::Mutex::new(lookat::storage::Store::open(&data)?));
    println!(
        "Lookat Chamber listening on http://{}; Formicary API-key ingestion enabled",
        listener.local_addr()?
    );
    axum::serve(listener, lookat::web::router(frontend, store))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
