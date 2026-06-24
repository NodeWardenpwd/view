use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use futures::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;

const DEFAULT_AKTOOLS_URL: &str = "https://vwpypm0t-kangupiaodeapi.hf.space";
const SHANGHAI_OFFSET_SECS: i32 = 8 * 3600;

pub struct TradingViewState {
    pub http: Client,
    pub aktools_url: String,
    pub symbol_cache: Arc<RwLock<Vec<StockEntry>>>,
}

#[derive(Clone, Debug)]
pub struct StockEntry {
    code: String,
    name: String,
    exchange: String,
    tv_symbol: String,
}

impl TradingViewState {
    pub fn new() -> Self {
        let aktools_url = std::env::var("AKTOOLS_URL")
            .unwrap_or_else(|_| DEFAULT_AKTOOLS_URL.to_string())
            .trim_end_matches('/')
            .to_string();

        info!("AKTools proxy base URL: {}", aktools_url);

        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("Failed to build HTTP client"),
            aktools_url,
            symbol_cache: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn get_tracked_symbols_from_env() -> Vec<String> {
        std::env::var("TRACKED_SYMBOL")
            .unwrap_or_else(|_| "sh600519,sz000001,sz300750,sh601318".to_string())
            .split(',')
            .map(|s| normalize_tv_symbol(s.trim()))
            .filter(|s| !s.is_empty())
            .collect()
    }
}

pub fn tradingview_routes() -> Router<Arc<TradingViewState>> {
    Router::new()
        .route("/config", get(get_config))
        .route("/time", get(get_time))
        .route("/symbols", get(get_symbol_info))
        .route("/search", get(search_symbols))
        .route("/tracked-symbols", get(get_tracked_symbols))
        .route("/daily-opens", get(get_daily_opens))
        .route("/history", get(get_history))
        .route("/ws", get(ws_handler))
        .route("/canvas/list", get(canvas_list))
        .route("/canvas/load", get(canvas_load))
        .route("/canvas/save", post(canvas_save))
        .route("/canvas/delete", delete(canvas_delete))
}

// ============ AKTools Data Models ============

#[derive(Debug, Deserialize)]
struct AkStockBar {
    #[serde(alias = "日期")]
    date: String,
    #[serde(alias = "开盘")]
    open: f64,
    #[serde(alias = "收盘")]
    close: f64,
    #[serde(alias = "最高")]
    high: f64,
    #[serde(alias = "最低")]
    low: f64,
    #[serde(alias = "成交量")]
    volume: f64,
}

#[derive(Debug, Deserialize)]
struct AkStockNameRow {
    #[serde(alias = "code")]
    code: Option<String>,
    #[serde(alias = "name")]
    name: Option<String>,
    #[serde(alias = "代码")]
    code_cn: Option<String>,
    #[serde(alias = "名称")]
    name_cn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AkSpotRow {
    #[serde(alias = "代码")]
    code: Option<String>,
    #[serde(alias = "今开")]
    open: Option<f64>,
}

// ============ UDF Response Types ============

#[derive(Serialize)]
struct UdfConfig {
    supported_resolutions: Vec<&'static str>,
    supports_group_request: bool,
    supports_marks: bool,
    supports_search: bool,
    supports_timescale_marks: bool,
}

#[derive(Serialize)]
struct UdfSymbolInfo {
    symbol: String,
    ticker: String,
    name: String,
    full_name: String,
    description: String,
    exchange: String,
    listed_exchange: String,
    #[serde(rename = "type")]
    symbol_type: String,
    currency_code: String,
    session: String,
    timezone: String,
    minmovement: i32,
    minmov: i32,
    minmovement2: i32,
    minmov2: i32,
    pricescale: i64,
    supported_resolutions: Vec<&'static str>,
    has_intraday: bool,
    has_daily: bool,
    has_weekly_and_monthly: bool,
    data_status: String,
}

#[derive(Serialize)]
struct UdfSearchResult {
    symbol: String,
    full_name: String,
    description: String,
    exchange: String,
    ticker: String,
    #[serde(rename = "type")]
    symbol_type: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum UdfHistoryResponse {
    Ok {
        s: String,
        t: Vec<i64>,
        o: Vec<f64>,
        h: Vec<f64>,
        l: Vec<f64>,
        c: Vec<f64>,
        v: Vec<f64>,
    },
    NoData {
        s: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "nextTime")]
        next_time: Option<i64>,
    },
    Error {
        s: String,
        errmsg: String,
    },
}

#[derive(Deserialize)]
struct SymbolQuery {
    symbol: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    query: Option<String>,
    #[serde(rename = "type")]
    symbol_type: Option<String>,
    exchange: Option<String>,
    limit: Option<i32>,
}

#[derive(Deserialize)]
struct HistoryQuery {
    symbol: String,
    resolution: String,
    from: i64,
    to: i64,
    countback: Option<i64>,
}

// ============ Symbol Helpers ============

fn normalize_tv_symbol(symbol: &str) -> String {
    let lower = symbol.trim().to_lowercase();
    let code = extract_code_6(&lower);
    if code.len() != 6 {
        return lower;
    }
    if lower.starts_with("sh") || lower.starts_with("sz") {
        return format!("{}{}", &lower[..2], code);
    }
    if code.starts_with('6') {
        format!("sh{code}")
    } else {
        format!("sz{code}")
    }
}

fn extract_code_6(symbol: &str) -> String {
    let digits: String = symbol.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 6 {
        digits[digits.len() - 6..].to_string()
    } else {
        digits
    }
}

fn exchange_for_code(code: &str) -> (&'static str, &'static str) {
    if code.starts_with('6') {
        ("SSE", "sh")
    } else {
        ("SZSE", "sz")
    }
}

fn build_stock_entry(code: String, name: String) -> StockEntry {
    let (exchange, prefix) = exchange_for_code(&code);
    StockEntry {
        code: code.clone(),
        name: name.clone(),
        exchange: exchange.to_string(),
        tv_symbol: format!("{prefix}{code}"),
    }
}

fn unix_to_yyyymmdd(ts: i64) -> String {
    let dt = DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_else(Utc::now);
    dt.format("%Y%m%d").to_string()
}

fn parse_bar_timestamp(date_str: &str) -> Option<i64> {
    let date_part = date_str.split('T').next()?.trim();

    if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
        let shanghai = FixedOffset::east_opt(SHANGHAI_OFFSET_SECS)?;
        return shanghai
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .single()
            .map(|dt| dt.timestamp());
    }

    if date_part.len() == 8 && date_part.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y%m%d") {
            let shanghai = FixedOffset::east_opt(SHANGHAI_OFFSET_SECS)?;
            return shanghai
                .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
                .single()
                .map(|dt| dt.timestamp());
        }
    }

    None
}

fn resolution_to_period(resolution: &str) -> Option<&'static str> {
    match resolution {
        "D" | "1D" => Some("daily"),
        "W" | "1W" => Some("weekly"),
        "M" | "1M" => Some("monthly"),
        _ => None,
    }
}

fn build_symbol_info(tv_symbol: &str, name: Option<String>) -> UdfSymbolInfo {
    let normalized = normalize_tv_symbol(tv_symbol);
    let code = extract_code_6(&normalized);
    let (exchange, _) = exchange_for_code(&code);
    let display_name = name.unwrap_or_else(|| code.clone());

    UdfSymbolInfo {
        symbol: normalized.clone(),
        ticker: normalized.clone(),
        name: display_name.clone(),
        full_name: format!("{exchange}:{code}"),
        description: display_name,
        exchange: exchange.to_string(),
        listed_exchange: exchange.to_string(),
        symbol_type: "stock".to_string(),
        currency_code: "CNY".to_string(),
        session: "0930-1500".to_string(),
        timezone: "Asia/Shanghai".to_string(),
        minmovement: 1,
        minmov: 1,
        minmovement2: 0,
        minmov2: 0,
        pricescale: 100,
        supported_resolutions: vec!["1D", "1W", "1M"],
        has_intraday: false,
        has_daily: true,
        has_weekly_and_monthly: true,
        data_status: "endofday".to_string(),
    }
}

async fn load_symbol_directory(state: &TradingViewState) -> Result<Vec<StockEntry>, String> {
    {
        let cache = state.symbol_cache.read().await;
        if !cache.is_empty() {
            return Ok(cache.clone());
        }
    }

    let url = format!("{}/api/public/stock_info_a_code_name", state.aktools_url);
    let resp = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("AKTools symbol list request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("AKTools symbol list HTTP {}", resp.status()));
    }

    let rows: Vec<AkStockNameRow> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse AKTools symbol list: {e}"))?;

    let entries: Vec<StockEntry> = rows
        .into_iter()
        .filter_map(|row| {
            let code = row.code.or(row.code_cn)?;
            let name = row.name.or(row.name_cn)?;
            if code.len() == 6 {
                Some(build_stock_entry(code, name))
            } else {
                None
            }
        })
        .collect();

    {
        let mut cache = state.symbol_cache.write().await;
        *cache = entries.clone();
    }

    info!("Loaded {} A-share symbols from AKTools", entries.len());
    Ok(entries)
}

async fn find_stock_entry(state: &TradingViewState, tv_symbol: &str) -> Option<StockEntry> {
    let normalized = normalize_tv_symbol(tv_symbol);
    let code = extract_code_6(&normalized);

    if let Ok(entries) = load_symbol_directory(state).await {
        if let Some(entry) = entries.iter().find(|e| e.code == code) {
            return Some(entry.clone());
        }
    }

    Some(build_stock_entry(code.clone(), code))
}

async fn fetch_history_bars(
    state: &TradingViewState,
    code: &str,
    period: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<AkStockBar>, String> {
    let url = format!(
        "{}/api/public/stock_zh_a_hist?symbol={code}&period={period}&start_date={start_date}&end_date={end_date}&adjust=qfq",
        state.aktools_url
    );

    debug!("Fetching AKTools history: {}", url);

    let resp = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("AKTools history request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("AKTools history HTTP {}", resp.status()));
    }

    resp.json::<Vec<AkStockBar>>()
        .await
        .map_err(|e| format!("Failed to parse AKTools history JSON: {e}"))
}

// ============ Handlers ============

async fn get_config() -> Json<UdfConfig> {
    Json(UdfConfig {
        supported_resolutions: vec!["1D", "1W", "1M"],
        supports_group_request: false,
        supports_marks: false,
        supports_search: true,
        supports_timescale_marks: false,
    })
}

async fn get_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string()
}

async fn get_symbol_info(
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<SymbolQuery>,
) -> Json<UdfSymbolInfo> {
    let entry = find_stock_entry(&state, &query.symbol).await;
    let info = match entry {
        Some(e) => build_symbol_info(&e.tv_symbol, Some(e.name)),
        None => build_symbol_info(&query.symbol, None),
    };
    Json(info)
}

async fn get_tracked_symbols() -> Json<Vec<String>> {
    Json(TradingViewState::get_tracked_symbols_from_env())
}

async fn get_daily_opens(
    State(state): State<Arc<TradingViewState>>,
) -> Json<std::collections::HashMap<String, f64>> {
    let mut opens = std::collections::HashMap::new();
    let symbols = TradingViewState::get_tracked_symbols_from_env();

    let url = format!("{}/api/public/stock_zh_a_spot_em", state.aktools_url);
    if let Ok(resp) = state.http.get(&url).send().await {
        if resp.status().is_success() {
            if let Ok(rows) = resp.json::<Vec<AkSpotRow>>().await {
                for row in rows {
                    if let (Some(code), Some(open)) = (row.code, row.open) {
                        let tv_symbol = normalize_tv_symbol(&code);
                        if symbols.contains(&tv_symbol) {
                            opens.insert(tv_symbol, open);
                        }
                    }
                }
            }
        }
    }

    Json(opens)
}

async fn search_symbols(
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<SearchQuery>,
) -> Json<Vec<UdfSearchResult>> {
    let search_term = query.query.unwrap_or_default().to_lowercase();
    let limit = query.limit.unwrap_or(30).max(1) as usize;

    let entries = match load_symbol_directory(&state).await {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Symbol search fallback due to: {}", e);
            TradingViewState::get_tracked_symbols_from_env()
                .into_iter()
                .map(|s| {
                    let code = extract_code_6(&s);
                    build_stock_entry(code, s.clone())
                })
                .collect()
        }
    };

    let results: Vec<UdfSearchResult> = entries
        .into_iter()
        .filter(|entry| {
            if search_term.is_empty() {
                return true;
            }
            entry.code.contains(&search_term)
                || entry.name.to_lowercase().contains(&search_term)
                || entry.tv_symbol.contains(&search_term)
        })
        .take(limit)
        .map(|entry| UdfSearchResult {
            symbol: entry.tv_symbol.clone(),
            full_name: format!("{}:{}", entry.exchange, entry.code),
            description: format!("{} {}", entry.name, entry.code),
            exchange: entry.exchange.clone(),
            ticker: entry.tv_symbol,
            symbol_type: "stock".to_string(),
        })
        .collect();

    Json(results)
}

async fn get_history(
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<HistoryQuery>,
) -> Json<UdfHistoryResponse> {
    let period = match resolution_to_period(&query.resolution) {
        Some(p) => p,
        None => {
            return Json(UdfHistoryResponse::NoData {
                s: "no_data".to_string(),
                next_time: None,
            });
        }
    };

    let code = extract_code_6(&query.symbol);
    if code.len() != 6 {
        return Json(UdfHistoryResponse::Error {
            s: "error".to_string(),
            errmsg: format!("Invalid A-share symbol: {}", query.symbol),
        });
    }

    let start_date = unix_to_yyyymmdd(query.from);
    let end_date = unix_to_yyyymmdd(query.to);

    match fetch_history_bars(&state, &code, period, &start_date, &end_date).await {
        Ok(bars) => {
            let mut parsed: Vec<(i64, &AkStockBar)> = bars
                .iter()
                .filter_map(|bar| parse_bar_timestamp(&bar.date).map(|ts| (ts, bar)))
                .filter(|(ts, _)| *ts >= query.from && *ts <= query.to)
                .collect();

            parsed.sort_by_key(|(ts, _)| *ts);

            if let Some(countback) = query.countback {
                if parsed.len() > countback as usize {
                    parsed = parsed.split_off(parsed.len() - countback as usize);
                }
            }

            if parsed.is_empty() {
                return Json(UdfHistoryResponse::NoData {
                    s: "no_data".to_string(),
                    next_time: None,
                });
            }

            let mut t = Vec::with_capacity(parsed.len());
            let mut o = Vec::with_capacity(parsed.len());
            let mut h = Vec::with_capacity(parsed.len());
            let mut l = Vec::with_capacity(parsed.len());
            let mut c = Vec::with_capacity(parsed.len());
            let mut v = Vec::with_capacity(parsed.len());

            for (ts, bar) in parsed {
                t.push(ts);
                o.push(bar.open);
                h.push(bar.high);
                l.push(bar.low);
                c.push(bar.close);
                v.push(bar.volume);
            }

            Json(UdfHistoryResponse::Ok {
                s: "ok".to_string(),
                t,
                o,
                h,
                l,
                c,
                v,
            })
        }
        Err(e) => Json(UdfHistoryResponse::Error {
            s: "error".to_string(),
            errmsg: e,
        }),
    }
}

// ============ WebSocket Handler (kept for frontend compatibility) ============

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_ws_connection)
}

async fn handle_ws_connection(mut socket: WebSocket) {
    info!("WebSocket client connected (A-share proxy mode: no live stream)");

    while let Some(msg) = socket.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if text.contains("\"ping\"") || text.contains("Ping") {
                    let _ = socket
                        .send(Message::Text(r#"{"type":"pong"}"#.into()))
                        .await;
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                warn!("WebSocket receive error: {}", e);
                break;
            }
            _ => {}
        }
    }

    info!("WebSocket connection closed");
}

// ============ Canvas API ============

const STORAGE_DIR: &str = "storage";
const DEFAULT_USER: &str = "default";

#[derive(Deserialize)]
struct CanvasListQuery {
    symbol: String,
}

#[derive(Deserialize)]
struct CanvasLoadQuery {
    symbol: String,
    name: String,
}

#[derive(Deserialize)]
struct CanvasSaveBody {
    symbol: String,
    name: String,
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct CanvasDeleteQuery {
    symbol: String,
    name: String,
}

#[derive(Serialize)]
struct CanvasListResponse {
    canvases: Vec<String>,
}

fn get_user_id(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("X-User-Id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_USER)
        .to_string()
}

fn get_canvas_dir(user_id: &str, symbol: &str) -> PathBuf {
    PathBuf::from(STORAGE_DIR)
        .join(user_id)
        .join(normalize_tv_symbol(symbol))
}

fn get_canvas_path(user_id: &str, symbol: &str, name: &str) -> PathBuf {
    get_canvas_dir(user_id, symbol).join(format!("{name}.json"))
}

async fn canvas_list(
    headers: axum::http::HeaderMap,
    Query(query): Query<CanvasListQuery>,
) -> Json<CanvasListResponse> {
    let user_id = get_user_id(&headers);
    let dir = get_canvas_dir(&user_id, &query.symbol);
    let mut canvases = Vec::new();

    if let Ok(mut entries) = fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    canvases.push(name.trim_end_matches(".json").to_string());
                }
            }
        }
    }

    canvases.sort();
    Json(CanvasListResponse { canvases })
}

async fn canvas_load(
    headers: axum::http::HeaderMap,
    Query(query): Query<CanvasLoadQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let user_id = get_user_id(&headers);
    let path = get_canvas_path(&user_id, &query.symbol, &query.name);

    match fs::read_to_string(&path).await {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(data) => Ok(Json(data)),
            Err(e) => {
                error!("Failed to parse canvas {}: {}", path.display(), e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

async fn canvas_save(
    headers: axum::http::HeaderMap,
    Json(body): Json<CanvasSaveBody>,
) -> Result<StatusCode, StatusCode> {
    let user_id = get_user_id(&headers);
    let dir = get_canvas_dir(&user_id, &body.symbol);
    let path = get_canvas_path(&user_id, &body.symbol, &body.name);

    if let Err(e) = fs::create_dir_all(&dir).await {
        error!("Failed to create dir {}: {}", dir.display(), e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let content = serde_json::to_string_pretty(&body.data)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    fs::write(&path, content).await.map_err(|e| {
        error!("Failed to write canvas {}: {}", path.display(), e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Saved canvas: {}", path.display());
    Ok(StatusCode::OK)
}

async fn canvas_delete(
    headers: axum::http::HeaderMap,
    Query(query): Query<CanvasDeleteQuery>,
) -> StatusCode {
    let user_id = get_user_id(&headers);
    let path = get_canvas_path(&user_id, &query.symbol, &query.name);

    match fs::remove_file(&path).await {
        Ok(_) => {
            info!("Deleted canvas: {}", path.display());
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_FOUND,
    }
}
