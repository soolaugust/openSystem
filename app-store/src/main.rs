use app_store::{
    registry::AppRegistry,
    server::{create_router, AppState},
};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let db_path = std::env::var("OPENSYSTEM_STORE_DB")
        .unwrap_or_else(|_| "/var/lib/opensystem/store.db".into());
    let store_dir =
        std::env::var("OPENSYSTEM_STORE_DIR").unwrap_or_else(|_| "/var/lib/opensystem/apps".into());

    std::fs::create_dir_all(&store_dir)?;

    let registry = AppRegistry::new(&db_path, &store_dir)?;
    let state = AppState {
        registry: Arc::new(Mutex::new(registry)),
        store_dir: store_dir.into(),
    };

    let app = create_router(state);
    let port = std::env::var("OPENSYSTEM_STORE_PORT").unwrap_or_else(|_| "8888".into());
    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("App store server listening on :{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
