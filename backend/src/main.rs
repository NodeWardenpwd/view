use backend::{tradingview_routes, TradingViewState};

use axum::Router;
use axum::http::Method;
use log::info;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    info!("A-Share TradingView UDF Proxy Server v2.0");

    let tradingview_state = Arc::new(TradingViewState::new());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .merge(tradingview_routes().with_state(tradingview_state))
        .layer(cors);

    let addr = "0.0.0.0:3000";
    info!("Starting API server on {}", addr);
    info!("TradingView UDF: /config, /symbols, /search, /history, /time");
    info!("Data source: East Money public API");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
