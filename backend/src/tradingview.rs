use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use futures::StreamExt;
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::sleep;

const SHANGHAI_OFFSET_SECS: i32 = 8 * 3600;
const SUPPORTED_RESOLUTIONS: &[&str] = &["1", "5", "15", "30", "60", "D", "W", "M"];
const EASTMONEY_KLINE_URL: &str = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
const EASTMONEY_QUOTE_URL: &str = "https://push2.eastmoney.com/api/qt/ulist.np/get";
const EASTMONEY_LIST_URL: &str = "https://push2.eastmoney.com/api/qt/clist/get";
const EASTMONEY_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const EASTMONEY_MAX_RETRIES: u32 = 3;
const EASTMONEY_RETRY_DELAY_MS: u64 = 500;

pub struct TradingViewState {
    pub http: Client,
    symbol_cache: Arc<RwLock<Vec<StockEntry>>>,
}

#[derive(Clone, Debug)]
struct StockEntry {
    code: String,
    name: String,
    exchange: String,
    tv_symbol: String,
}

#[derive(Debug, Clone)]
struct StockBar {
    date: String,
    open: f64,
    close: f64,
    high: f64,
    low: f64,
    volume: f64,
}

impl TradingViewState {
    pub fn new() -> Self {
        info!("TradingView UDF proxy initialized (East Money data source)");
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to build HTTP client"),
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
        .route("/quotes", get(get_quotes))
        .route("/history", get(get_history))
        .route("/ws", get(ws_handler))
        .route("/canvas/list", get(canvas_list))
        .route("/canvas/load", get(canvas_load))
        .route("/canvas/save", post(canvas_save))
        .route("/canvas/delete", delete(canvas_delete))
        .route("/auth/verify", get(verify_email))
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

#[derive(Serialize)]
struct QuoteItem {
    symbol: String,
    last: f64,
    change: f64,
    change_percent: f64,
    open: f64,
}

#[derive(Deserialize)]
struct SymbolQuery {
    symbol: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    query: Option<String>,
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

#[derive(Deserialize)]
struct QuotesQuery {
    symbols: String,
}

#[derive(Deserialize)]
pub struct VerifyParams {
    pub email: String,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub allowed: bool,
}

// ============ Auth Whitelist ============

pub fn check_email_allowed(email: &str) -> bool {
    if std::env::var("AUTH_DISABLED")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
    {
        return true;
    }

    let email = email.trim().to_lowercase();
    if email.is_empty() {
        return false;
    }

    if let Ok(list) = std::env::var("ALLOWED_EMAILS") {
        for allowed in list.split(',') {
            if allowed.trim().to_lowercase() == email {
                return true;
            }
        }
    }

    if let Ok(single) = std::env::var("ALLOWED_EMAIL") {
        if !single.trim().is_empty() && single.trim().to_lowercase() == email {
            return true;
        }
    }

    false
}

fn extract_email_from_request(headers: &HeaderMap) -> Option<String> {
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookies) = cookie_header.to_str() {
            for part in cookies.split(';') {
                let part = part.trim();
                if let Some(raw) = part.strip_prefix("logged_in_email=") {
                    let email = url_decode(raw);
                    if !email.is_empty() {
                        return Some(email);
                    }
                }
            }
        }
    }

    if let Some(value) = headers.get("x-user-email").and_then(|v| v.to_str().ok()) {
        if !value.trim().is_empty() {
            return Some(value.trim().to_string());
        }
    }

    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            if let Some(email) = email_from_jwt(token.trim()) {
                return Some(email);
            }
        }
    }

    None
}

fn url_decode(input: &str) -> String {
    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn email_from_jwt(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let mut b64 = payload_b64.replace('-', "+").replace('_', "/");
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    let bytes = base64_decode(&b64).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 128] = &{
        let mut table = [255u8; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i;
            table[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            table[(b'0' + d) as usize] = d + 52;
            d += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let input = input.as_bytes();
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;

    for &byte in input {
        if byte == b'=' {
            break;
        }
        let val = if (byte as usize) < 128 {
            TABLE[byte as usize]
        } else {
            255
        };
        if val == 255 {
            continue;
        }
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    if out.is_empty() {
        Err(())
    } else {
        Ok(out)
    }
}

fn ensure_authorized(headers: &HeaderMap) -> Result<(), Response> {
    if std::env::var("AUTH_DISABLED")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
    {
        return Ok(());
    }

    match extract_email_from_request(headers) {
        Some(email) if check_email_allowed(&email) => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED.into_response()),
    }
}

fn unauthorized_history() -> Json<UdfHistoryResponse> {
    Json(UdfHistoryResponse::Error {
        s: "error".to_string(),
        errmsg: "Unauthorized Account".to_string(),
    })
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

fn eastmoney_secid(code: &str) -> String {
    let clean_code = extract_code_6(code);
    if clean_code.starts_with('6') {
        format!("1.{clean_code}")
    } else {
        format!("0.{clean_code}")
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
    DateTime::<Utc>::from_timestamp(ts, 0)
        .unwrap_or_else(Utc::now)
        .format("%Y%m%d")
        .to_string()
}

fn parse_bar_timestamp(date_str: &str) -> Option<i64> {
    let value = date_str.trim();
    let shanghai = FixedOffset::east_opt(SHANGHAI_OFFSET_SECS)?;

    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M") {
        return shanghai
            .from_local_datetime(&dt)
            .single()
            .map(|dt| dt.timestamp());
    }

    let date_part = value.split('T').next()?.trim();

    if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
        return shanghai
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .single()
            .map(|dt| dt.timestamp());
    }

    if date_part.len() == 8 && date_part.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y%m%d") {
            return shanghai
                .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
                .single()
                .map(|dt| dt.timestamp());
        }
    }

    None
}

fn resolution_to_klt(resolution: &str) -> Option<u32> {
    match resolution {
        "1" => Some(1),
        "5" => Some(5),
        "15" => Some(15),
        "30" => Some(30),
        "60" => Some(60),
        "D" | "1D" => Some(101),
        "W" | "1W" => Some(102),
        "M" | "1M" => Some(103),
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
        supported_resolutions: SUPPORTED_RESOLUTIONS.to_vec(),
        has_intraday: true,
        has_daily: true,
        has_weekly_and_monthly: true,
        data_status: "streaming".to_string(),
    }
}

fn parse_eastmoney_kline(line: &str) -> Option<StockBar> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 6 {
        return None;
    }
    Some(StockBar {
        date: parts[0].to_string(),
        open: parts[1].parse().unwrap_or(0.0),
        close: parts[2].parse().unwrap_or(0.0),
        high: parts[3].parse().unwrap_or(0.0),
        low: parts[4].parse().unwrap_or(0.0),
        volume: parts[5].parse().unwrap_or(0.0),
    })
}

fn eastmoney_request(client: &Client, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header("User-Agent", EASTMONEY_UA)
        .header("Referer", "https://quote.eastmoney.com/")
        .header("Accept", "*/*")
        .header("Accept-Language", "zh-CN,zh;q=0.9")
}

async fn eastmoney_get_with_retry(
    client: &Client,
    url: &str,
) -> Result<reqwest::Response, String> {
    let mut last_err = "EastMoney request failed: unknown error".to_string();

    for attempt in 0..EASTMONEY_MAX_RETRIES {
        if attempt > 0 {
            sleep(Duration::from_millis(EASTMONEY_RETRY_DELAY_MS)).await;
        }

        match eastmoney_request(client, url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                last_err = format!("EastMoney HTTP {}", resp.status());
                warn!(
                    "EastMoney request attempt {}/{} got HTTP {}",
                    attempt + 1,
                    EASTMONEY_MAX_RETRIES,
                    resp.status()
                );
            }
            Err(e) => {
                last_err = format!("EastMoney request failed: {e}");
                warn!(
                    "EastMoney request attempt {}/{} failed: {e}",
                    attempt + 1,
                    EASTMONEY_MAX_RETRIES
                );
            }
        }
    }

    Err(last_err)
}

async fn load_symbol_directory(state: &TradingViewState) -> Result<Vec<StockEntry>, String> {
    {
        let cache = state.symbol_cache.read().await;
        if !cache.is_empty() {
            return Ok(cache.clone());
        }
    }

    let url = format!(
        "{EASTMONEY_LIST_URL}?pn=1&pz=5000&po=1&np=1&fltt=2&invt=2&fid=f12&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23&fields=f12,f14"
    );

    let resp = eastmoney_get_with_retry(&state.http, &url)
        .await
        .map_err(|e| format!("EastMoney symbol list request failed: {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse EastMoney symbol list: {e}"))?;

    let entries: Vec<StockEntry> = json["data"]["diff"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|row| {
            let code = row["f12"].as_str()?.trim();
            let name = row["f14"].as_str()?.trim();
            if code.len() == 6 {
                Some(build_stock_entry(code.to_string(), name.to_string()))
            } else {
                None
            }
        })
        .collect();

    {
        let mut cache = state.symbol_cache.write().await;
        *cache = entries.clone();
    }

    info!("Loaded {} A-share symbols from East Money", entries.len());
    Ok(entries)
}

async fn find_stock_entry(state: &TradingViewState, tv_symbol: &str) -> StockEntry {
    let normalized = normalize_tv_symbol(tv_symbol);
    let code = extract_code_6(&normalized);

    if let Ok(entries) = load_symbol_directory(state).await {
        if let Some(entry) = entries.iter().find(|e| e.code == code) {
            return entry.clone();
        }
    }

    build_stock_entry(code.clone(), code)
}

async fn fetch_history_bars(
    state: &TradingViewState,
    code: &str,
    klt: u32,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<StockBar>, String> {
    let clean_code = extract_code_6(code);

    if clean_code.len() != 6 {
        return Err(format!("Invalid A-share code: {code}"));
    }

    let secid = eastmoney_secid(&clean_code);
    let beg = start_date.replace('-', "");
    let end = end_date.replace('-', "");

    let url = format!(
        "{EASTMONEY_KLINE_URL}?secid={secid}&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&klt={klt}&fqt=1&beg={beg}&end={end}"
    );

    debug!("EastMoney kline request: {}", url);

    let resp = eastmoney_get_with_retry(&state.http, &url).await?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse EastMoney JSON: {e}"))?;

    let klines = json["data"]["klines"]
        .as_array()
        .ok_or_else(|| "EastMoney response missing data.klines".to_string())?;

    let bars: Vec<StockBar> = klines
        .iter()
        .filter_map(|item| item.as_str())
        .filter_map(parse_eastmoney_kline)
        .collect();

    if bars.is_empty() {
        return Err("EastMoney returned empty klines".to_string());
    }

    Ok(bars)
}

async fn fetch_quotes(
    state: &TradingViewState,
    tv_symbols: &[String],
) -> HashMap<String, QuoteItem> {
    if tv_symbols.is_empty() {
        return HashMap::new();
    }

    let secids: Vec<String> = tv_symbols
        .iter()
        .map(|symbol| eastmoney_secid(&extract_code_6(symbol)))
        .collect();

    let url = format!(
        "{EASTMONEY_QUOTE_URL}?fltt=2&fields=f2,f3,f4,f46,f12,f14&secids={}",
        secids.join(",")
    );

    let mut result = HashMap::new();

    let Ok(resp) = eastmoney_get_with_retry(&state.http, &url).await else {
        return result;
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return result;
    };

    if let Some(items) = json["data"]["diff"].as_array().or_else(|| json["data"]["full"].as_array()) {
        for item in items {
            let code = item["f12"].as_str().unwrap_or_default();
            if code.len() != 6 {
                continue;
            }
            let tv_symbol = normalize_tv_symbol(code);
            let last = item["f2"].as_f64().unwrap_or(0.0);
            let change_percent = item["f3"].as_f64().unwrap_or(0.0);
            let change = item["f4"].as_f64().unwrap_or(0.0);
            let open = item["f46"].as_f64().unwrap_or(last - change);
            result.insert(
                tv_symbol.clone(),
                QuoteItem {
                    symbol: tv_symbol,
                    last,
                    change,
                    change_percent,
                    open,
                },
            );
        }
    }

    result
}

// ============ Handlers ============

async fn get_config(headers: HeaderMap) -> Result<Json<UdfConfig>, Response> {
    ensure_authorized(&headers)?;
    Ok(Json(UdfConfig {
        supported_resolutions: SUPPORTED_RESOLUTIONS.to_vec(),
        supports_group_request: false,
        supports_marks: false,
        supports_search: true,
        supports_timescale_marks: false,
    }))
}

async fn get_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string()
}

async fn get_symbol_info(
    headers: HeaderMap,
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<SymbolQuery>,
) -> Result<Json<UdfSymbolInfo>, Response> {
    ensure_authorized(&headers)?;
    let entry = find_stock_entry(&state, &query.symbol).await;
    Ok(Json(build_symbol_info(
        &entry.tv_symbol,
        Some(entry.name),
    )))
}

async fn get_tracked_symbols() -> Json<Vec<String>> {
    Json(TradingViewState::get_tracked_symbols_from_env())
}

async fn get_daily_opens(
    State(state): State<Arc<TradingViewState>>,
) -> Json<HashMap<String, f64>> {
    let symbols = TradingViewState::get_tracked_symbols_from_env();
    let quotes = fetch_quotes(&state, &symbols).await;
    let opens = quotes
        .into_iter()
        .map(|(symbol, quote)| (symbol, quote.open))
        .collect();
    Json(opens)
}

async fn get_quotes(
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<QuotesQuery>,
) -> Json<HashMap<String, QuoteItem>> {
    let symbols: Vec<String> = query
        .symbols
        .split(',')
        .map(|s| normalize_tv_symbol(s.trim()))
        .filter(|s| !s.is_empty())
        .collect();
    Json(fetch_quotes(&state, &symbols).await)
}

async fn search_symbols(
    headers: HeaderMap,
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<UdfSearchResult>>, Response> {
    ensure_authorized(&headers)?;

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
            search_term.is_empty()
                || entry.code.contains(&search_term)
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

    Ok(Json(results))
}

async fn get_history(
    headers: HeaderMap,
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<UdfHistoryResponse>, Response> {
    if ensure_authorized(&headers).is_err() {
        return Ok(unauthorized_history());
    }

    let klt = match resolution_to_klt(&query.resolution) {
        Some(klt) => klt,
        None => {
            return Ok(Json(UdfHistoryResponse::NoData {
                s: "no_data".to_string(),
                next_time: None,
            }));
        }
    };

    let code = extract_code_6(&query.symbol);
    if code.len() != 6 {
        return Ok(Json(UdfHistoryResponse::Error {
            s: "error".to_string(),
            errmsg: format!("Invalid A-share symbol: {}", query.symbol),
        }));
    }

    let start_date = unix_to_yyyymmdd(query.from);
    let end_date = unix_to_yyyymmdd(query.to);

    match fetch_history_bars(&state, &code, klt, &start_date, &end_date).await {
        Ok(bars) => {
            let mut parsed: Vec<(i64, &StockBar)> = bars
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
                return Ok(Json(UdfHistoryResponse::NoData {
                    s: "no_data".to_string(),
                    next_time: None,
                }));
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

            Ok(Json(UdfHistoryResponse::Ok {
                s: "ok".to_string(),
                t,
                o,
                h,
                l,
                c,
                v,
            }))
        }
        Err(e) => Ok(Json(UdfHistoryResponse::Error {
            s: "error".to_string(),
            errmsg: e,
        })),
    }
}

pub async fn verify_email(Query(params): Query<VerifyParams>) -> Json<VerifyResponse> {
    Json(VerifyResponse {
        allowed: check_email_allowed(&params.email),
    })
}

// ============ WebSocket Handler ============

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_ws_connection)
}

async fn handle_ws_connection(mut socket: WebSocket) {
    info!("WebSocket client connected (polling mode recommended)");

    while let Some(msg) = socket.next().await {
        match msg {
            Ok(Message::Text(text)) if text.contains("ping") || text.contains("Ping") => {
                let _ = socket.send(Message::Text(r#"{"type":"pong"}"#.into())).await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
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

fn get_user_id(headers: &HeaderMap) -> String {
    extract_email_from_request(headers)
        .or_else(|| {
            headers
                .get("X-User-Id")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| DEFAULT_USER.to_string())
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
    headers: HeaderMap,
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
    headers: HeaderMap,
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
    headers: HeaderMap,
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
    headers: HeaderMap,
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
