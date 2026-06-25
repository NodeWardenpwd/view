use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use chrono::{FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use encoding_rs::GBK;
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
const TENCENT_FQKLINE_URL: &str = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";
const TENCENT_MKLINE_URL: &str = "https://ifzq.gtimg.cn/appstock/app/kline/mkline";
const TENCENT_KLINE_LIMIT: u32 = 1000;
const TENCENT_QT_URL: &str = "https://qt.gtimg.cn/q=";
const TENCENT_SEARCH_URL: &str = "https://proxy.finance.qq.com/ifzqgtimg/appstock/code/search";
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const HTTP_MAX_RETRIES: u32 = 3;
const HTTP_RETRY_DELAY_MS: u64 = 500;

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
        info!("TradingView UDF proxy initialized (Tencent Finance data source)");
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
    name: String,
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

#[derive(Debug, Clone)]
struct TencentQuote {
    tv_symbol: String,
    name: String,
    code: String,
    last: f64,
    open: f64,
    change: f64,
    change_percent: f64,
}

fn clean_code_digits(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 6 {
        digits[digits.len() - 6..].to_string()
    } else {
        digits
    }
}

fn extract_code_6(symbol: &str) -> String {
    clean_code_digits(symbol)
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

    if value.len() == 12 && value.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y%m%d%H%M") {
            return shanghai.from_local_datetime(&dt).single().map(|dt| dt.timestamp());
        }
    }

    None
}

fn normalize_resolution(resolution: &str) -> Option<&'static str> {
    match resolution {
        "1" => Some("1"),
        "5" => Some("5"),
        "15" => Some("15"),
        "30" => Some("30"),
        "60" => Some("60"),
        "D" | "1D" => Some("D"),
        "W" | "1W" => Some("W"),
        "M" | "1M" => Some("M"),
        _ => None,
    }
}

fn tencent_market(clean_code: &str) -> &'static str {
    if clean_code.starts_with('6') {
        "sh"
    } else {
        "sz"
    }
}

fn tencent_symbol(market: &str, clean_code: &str) -> String {
    format!("{market}{clean_code}")
}

fn tencent_kline_url(market: &str, clean_code: &str, period: &str) -> Option<String> {
    let symbol = tencent_symbol(market, clean_code);
    if matches!(period, "1" | "5" | "15" | "30" | "60") {
        Some(format!(
            "{TENCENT_MKLINE_URL}?param={symbol},m{period},,{TENCENT_KLINE_LIMIT}"
        ))
    } else {
        let unit = match period {
            "D" | "d" => "day",
            "W" | "w" => "week",
            "M" | "m" => "month",
            _ => return None,
        };
        Some(format!(
            "{TENCENT_FQKLINE_URL}?param={symbol},{unit},,,{TENCENT_KLINE_LIMIT},qfq"
        ))
    }
}

fn tencent_data_key(period: &str) -> String {
    match period {
        "1" | "5" | "15" | "30" | "60" => format!("m{period}"),
        "D" | "d" => "qfqday".to_string(),
        "W" | "w" => "qfqweek".to_string(),
        "M" | "m" => "qfqmonth".to_string(),
        other => other.to_string(),
    }
}

fn json_to_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .or_else(|| value.as_i64().map(|v| v as f64))
}

fn parse_tencent_bar(row: &serde_json::Value) -> Option<StockBar> {
    let items = row.as_array()?;
    if items.len() < 6 {
        return None;
    }
    Some(StockBar {
        date: items[0].as_str()?.to_string(),
        open: json_to_f64(&items[1])?,
        close: json_to_f64(&items[2])?,
        high: json_to_f64(&items[3])?,
        low: json_to_f64(&items[4])?,
        volume: json_to_f64(&items[5])?,
    })
}

fn extract_tencent_bars(json: &serde_json::Value, symbol: &str, data_key: &str) -> Vec<StockBar> {
    let data = &json["data"];

    let candidates = [
        data.get(symbol).and_then(|v| v.get(data_key)),
        data.get(data_key),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Some(rows) = candidate.as_array() {
            let bars: Vec<StockBar> = rows.iter().filter_map(parse_tencent_bar).collect();
            if !bars.is_empty() {
                return bars;
            }
        }
    }

    if let Some(obj) = data.as_object() {
        for value in obj.values() {
            if let Some(rows) = value.get(data_key).and_then(|v| v.as_array()) {
                let bars: Vec<StockBar> = rows.iter().filter_map(parse_tencent_bar).collect();
                if !bars.is_empty() {
                    return bars;
                }
            }
        }
    }

    Vec::new()
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

fn browser_request(client: &Client, url: &str) -> reqwest::RequestBuilder {
    client.get(url).header("User-Agent", BROWSER_UA)
}

async fn http_get_with_retry(client: &Client, url: &str, label: &str) -> Result<reqwest::Response, String> {
    let mut last_err = format!("{label} request failed: unknown error");

    for attempt in 0..HTTP_MAX_RETRIES {
        if attempt > 0 {
            sleep(Duration::from_millis(HTTP_RETRY_DELAY_MS)).await;
        }

        match browser_request(client, url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                last_err = format!("{label} HTTP {}", resp.status());
                warn!(
                    "{label} request attempt {}/{} got HTTP {}",
                    attempt + 1,
                    HTTP_MAX_RETRIES,
                    resp.status()
                );
            }
            Err(e) => {
                last_err = format!("{label} request failed: {e}");
                warn!(
                    "{label} request attempt {}/{} failed: {e}",
                    attempt + 1,
                    HTTP_MAX_RETRIES
                );
            }
        }
    }

    Err(last_err)
}

async fn http_get_gbk_text_with_retry(
    client: &Client,
    url: &str,
    label: &str,
) -> Result<String, String> {
    let resp = http_get_with_retry(client, url, label).await?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("{label} response read failed: {e}"))?;
    let (decoded, _, _) = GBK.decode(&bytes);
    Ok(decoded.into_owned())
}

fn parse_str_f64(value: &str) -> Option<f64> {
    if value.is_empty() {
        return None;
    }
    value.parse().ok()
}

fn parse_qt_line(line: &str) -> Option<TencentQuote> {
    let line = line.trim();
    let eq_pos = line.find('=')?;
    if !line.starts_with("v_") {
        return None;
    }

    let tv_symbol = line[2..eq_pos].trim().to_lowercase();
    let raw_value = line[eq_pos + 1..].trim().trim_matches('"');
    let fields: Vec<&str> = raw_value.split('~').collect();
    if fields.len() < 4 {
        return None;
    }

    let name = fields[1].trim().to_string();
    let code = fields[2].trim().to_string();
    let last = parse_str_f64(fields[3].trim())?;
    let prev_close = fields
        .get(4)
        .and_then(|v| parse_str_f64(v.trim()))
        .unwrap_or(last);
    let open = fields
        .get(5)
        .and_then(|v| parse_str_f64(v.trim()))
        .unwrap_or(prev_close);

    let change = last - prev_close;
    let change_percent = if prev_close.abs() > f64::EPSILON {
        change / prev_close * 100.0
    } else {
        0.0
    };

    Some(TencentQuote {
        tv_symbol: normalize_tv_symbol(&tv_symbol),
        name,
        code,
        last,
        open,
        change,
        change_percent,
    })
}

fn parse_qt_response(body: &str) -> HashMap<String, TencentQuote> {
    let mut quotes = HashMap::new();
    for line in body.lines() {
        if let Some(quote) = parse_qt_line(line) {
            quotes.insert(quote.tv_symbol.clone(), quote);
        }
    }
    quotes
}

fn tv_symbol_to_qt_param(tv_symbol: &str) -> Option<String> {
    let clean_code = clean_code_digits(tv_symbol);
    if clean_code.len() != 6 {
        return None;
    }
    Some(tencent_symbol(tencent_market(&clean_code), &clean_code))
}

async fn fetch_tencent_quotes(
    state: &TradingViewState,
    tv_symbols: &[String],
) -> HashMap<String, TencentQuote> {
    let qt_symbols: Vec<String> = tv_symbols
        .iter()
        .filter_map(|s| tv_symbol_to_qt_param(s))
        .collect();

    if qt_symbols.is_empty() {
        return HashMap::new();
    }

    let url = format!("{TENCENT_QT_URL}{}", qt_symbols.join(","));
    debug!("Tencent quote request: {}", url);

    let Ok(body) = http_get_gbk_text_with_retry(&state.http, &url, "Tencent quote").await else {
        return HashMap::new();
    };

    parse_qt_response(&body)
}

async fn fetch_tencent_symbol_entry(
    state: &TradingViewState,
    tv_symbol: &str,
) -> Option<StockEntry> {
    let normalized = normalize_tv_symbol(tv_symbol);
    let quotes = fetch_tencent_quotes(state, &[normalized.clone()]).await;
    quotes.get(&normalized).map(|q| {
        build_stock_entry(q.code.clone(), q.name.clone())
    })
}

async fn search_tencent_symbols(
    state: &TradingViewState,
    query: &str,
    limit: usize,
) -> Vec<StockEntry> {
    if query.is_empty() {
        return load_tracked_stock_entries(state).await;
    }

    let url = format!(
        "{TENCENT_SEARCH_URL}?keyword={}&market=&type=&page=1&count={limit}",
        urlencoding(query)
    );

    let Ok(resp) = http_get_with_retry(&state.http, &url, "Tencent symbol search").await else {
        return Vec::new();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    let arrays = [
        json["data"]["stock"].as_array(),
        json["data"]["all"].as_array(),
        json["data"].as_array(),
    ];

    for arr in arrays.into_iter().flatten() {
        for item in arr {
            let code_raw = item["code"]
                .as_str()
                .or_else(|| item["symbol"].as_str())
                .unwrap_or_default();
            let name = item["name"].as_str().unwrap_or_default().to_string();
            if code_raw.is_empty() {
                continue;
            }
            let tv_symbol = normalize_tv_symbol(code_raw);
            let clean_code = clean_code_digits(&tv_symbol);
            if clean_code.len() == 6 {
                entries.push(build_stock_entry(clean_code, name));
            }
            if entries.len() >= limit {
                return entries;
            }
        }
        if !entries.is_empty() {
            return entries;
        }
    }

    entries
}

fn urlencoding(input: &str) -> String {
    input
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn load_tracked_stock_entries(state: &TradingViewState) -> Vec<StockEntry> {
    {
        let cache = state.symbol_cache.read().await;
        if !cache.is_empty() {
            return cache.clone();
        }
    }

    let tracked = TradingViewState::get_tracked_symbols_from_env();
    let quotes = fetch_tencent_quotes(state, &tracked).await;

    let entries: Vec<StockEntry> = tracked
        .iter()
        .filter_map(|symbol| {
            quotes.get(symbol).map(|q| build_stock_entry(q.code.clone(), q.name.clone()))
        })
        .collect();

    if !entries.is_empty() {
        let mut cache = state.symbol_cache.write().await;
        *cache = entries.clone();
        info!("Loaded {} tracked symbols from Tencent Finance", entries.len());
    }

    entries
}

async fn find_stock_entry(state: &TradingViewState, tv_symbol: &str) -> StockEntry {
    if let Some(entry) = fetch_tencent_symbol_entry(state, tv_symbol).await {
        return entry;
    }

    let clean_code = clean_code_digits(tv_symbol);
    build_stock_entry(clean_code.clone(), clean_code)
}

async fn fetch_history_bars(
    state: &TradingViewState,
    code: &str,
    period: &str,
) -> Result<Vec<StockBar>, String> {
    let clean_code = clean_code_digits(code);

    if clean_code.len() != 6 {
        return Err(format!("Invalid A-share code: {code}"));
    }

    let market = tencent_market(&clean_code);
    let symbol = tencent_symbol(market, &clean_code);
    let url = tencent_kline_url(market, &clean_code, period)
        .ok_or_else(|| format!("Unsupported period: {period}"))?;
    let data_key = tencent_data_key(period);

    debug!("Tencent kline request: {}", url);

    let resp = http_get_with_retry(&state.http, &url, "Tencent kline").await?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Tencent JSON: {e}"))?;

    let bars = extract_tencent_bars(&json, &symbol, &data_key);

    if bars.is_empty() {
        return Err(format!(
            "Tencent returned empty klines for {symbol} ({data_key})"
        ));
    }

    Ok(bars)
}

async fn fetch_quotes(
    state: &TradingViewState,
    tv_symbols: &[String],
) -> HashMap<String, QuoteItem> {
    let quotes = fetch_tencent_quotes(state, tv_symbols).await;
    quotes
        .into_iter()
        .map(|(symbol, q)| {
            (
                symbol.clone(),
                QuoteItem {
                    symbol,
                    name: q.name,
                    last: q.last,
                    change: q.change,
                    change_percent: q.change_percent,
                    open: q.open,
                },
            )
        })
        .collect()
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

    let mut entries = search_tencent_symbols(&state, &search_term, limit).await;

    if entries.is_empty() {
        warn!("Tencent symbol search empty, falling back to tracked symbols");
        entries = TradingViewState::get_tracked_symbols_from_env()
            .into_iter()
            .map(|s| {
                let code = clean_code_digits(&s);
                build_stock_entry(code, s)
            })
            .collect();
    }

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

fn period_step_secs(period: &str) -> i64 {
    match period {
        "1" => 60,
        "5" => 300,
        "15" => 900,
        "30" => 1800,
        "60" => 3600,
        "D" | "d" => 86_400,
        "W" | "w" => 604_800,
        "M" | "m" => 2_592_000,
        _ => 86_400,
    }
}

fn should_remap_timestamps(from: i64, to: i64, parsed: &[(i64, &StockBar)]) -> bool {
    if parsed.is_empty() {
        return false;
    }
    if from < 0 || to < 0 || to < from {
        return true;
    }
    !parsed
        .iter()
        .any(|(ts, _)| *ts >= from && *ts <= to)
}

fn remap_timestamp_at(
    index: usize,
    count: usize,
    from: i64,
    to: i64,
    period: &str,
) -> i64 {
    if count == 0 {
        return from.max(to);
    }
    if count == 1 {
        return if to >= 0 { to } else { from.max(to) };
    }

    let step = period_step_secs(period);
    let last = count - 1;

    // Evenly distribute across a sane [from, to] window (e.g. TV asks for 2010 but data is 2026).
    if from >= 0 && to >= from {
        let span = to - from;
        return from + (index as i64 * span) / last as i64;
    }

    // Overflow / negative range: anchor last bar at `to`, walk backward by bar period.
    let anchor = if to != 0 { to } else { from + step * last as i64 };
    let offset = (last - index) as i64;
    anchor.saturating_sub(step * offset)
}

fn select_bars_for_history(
    bars: &[StockBar],
    countback: Option<i64>,
) -> Vec<(i64, &StockBar)> {
    let mut parsed: Vec<(i64, &StockBar)> = bars
        .iter()
        .filter_map(|bar| parse_bar_timestamp(&bar.date).map(|ts| (ts, bar)))
        .collect();

    parsed.sort_by_key(|(ts, _)| *ts);

    if parsed.is_empty() {
        return parsed;
    }

    // Never apply from/to filtering here — it can collapse to a single bar and break TV.
    // Always return the latest N bars; timestamp remapping handles the requested window.
    let min_bars = 2_usize;
    let requested = countback.map(|c| c.max(min_bars as i64) as usize);
    let take = requested.unwrap_or(300).max(min_bars).min(parsed.len());

    if parsed.len() > take {
        parsed = parsed.split_off(parsed.len() - take);
    }

    parsed
}

async fn get_history(
    headers: HeaderMap,
    State(state): State<Arc<TradingViewState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<UdfHistoryResponse>, Response> {
    if ensure_authorized(&headers).is_err() {
        return Ok(unauthorized_history());
    }

    let period = match normalize_resolution(&query.resolution) {
        Some(period) => period,
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

    match fetch_history_bars(&state, &code, period).await {
        Ok(bars) => {
            if bars.is_empty() {
                return Ok(Json(UdfHistoryResponse::NoData {
                    s: "no_data".to_string(),
                    next_time: None,
                }));
            }

            let parsed = select_bars_for_history(&bars, query.countback);

            if parsed.len() < 2 {
                warn!(
                    "Insufficient bars for {} ({}), returning no_data",
                    query.symbol,
                    parsed.len()
                );
                return Ok(Json(UdfHistoryResponse::NoData {
                    s: "no_data".to_string(),
                    next_time: None,
                }));
            }

            let remap = should_remap_timestamps(query.from, query.to, &parsed);
            if remap {
                warn!(
                    "Remapping {} {} bars into requested range from={} to={}",
                    parsed.len(),
                    period,
                    query.from,
                    query.to
                );
            }

            let mut t = Vec::with_capacity(parsed.len());
            let mut o = Vec::with_capacity(parsed.len());
            let mut h = Vec::with_capacity(parsed.len());
            let mut l = Vec::with_capacity(parsed.len());
            let mut c = Vec::with_capacity(parsed.len());
            let mut v = Vec::with_capacity(parsed.len());

            let bar_count = parsed.len();
            for (i, (real_ts, bar)) in parsed.iter().enumerate() {
                let ts = if remap {
                    remap_timestamp_at(i, bar_count, query.from, query.to, period)
                } else {
                    *real_ts
                };
                t.push(ts);
                o.push(bar.open);
                h.push(bar.high);
                l.push(bar.low);
                c.push(bar.close);
                v.push(bar.volume);
            }

            let len = t.len();
            if len < 2
                || o.len() != len
                || h.len() != len
                || l.len() != len
                || c.len() != len
                || v.len() != len
            {
                warn!("History array length mismatch for {}, returning no_data", query.symbol);
                return Ok(Json(UdfHistoryResponse::NoData {
                    s: "no_data".to_string(),
                    next_time: None,
                }));
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
        Err(e) => {
            warn!("History fetch failed for {}: {e}", query.symbol);
            Ok(Json(UdfHistoryResponse::NoData {
                s: "no_data".to_string(),
                next_time: None,
            }))
        }
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
