# 📊 View — A-Share Chart Platform v3.0

[🇨🇳 中文版 README](README_CN.md)

> **v3.0 Architecture Upgrade**
>
> 项目已升级至 v3.0，移除所有 AKTools 依赖。全面改用腾讯财经高速免签 API 驱动，秒回数据。原生支持分钟线、日线、周线、月线全周期图表，且天然免疫海外 IP 风控限制。

A self-hosted **A-share (China stock)** charting platform. A lightweight Rust Axum backend proxies **Tencent Finance** market data into the TradingView UDF protocol, paired with a professional Charting Library frontend.

## ✨ Features

### Market Data (v3.0)
- **Tencent Finance K-line API** — Intraday (1/5/15/30/60 min) + daily / weekly / monthly (qfq), all periods supported
- **Tencent Finance quote API** — GBK-decoded real-time quotes via `qt.gtimg.cn` for correct Chinese stock names
- **No local database required** — On-demand proxy; no TimescaleDB sync for chart data
- **Browser headers + retry** — Chrome User-Agent with 3-attempt retry for stable overseas deployments (e.g. Hugging Face)
- **Polling pseudo-realtime** — Frontend polls latest bars and watchlist quotes every 10–30 seconds

### Charting
- TradingView Charting Library with Simplified Chinese (`locale: "zh"`)
- Custom indicator adapter skeleton (Pine Script → JS extensible)
- Multi-chart layouts, chart save/load, dark/light themes

### Security
- Google OAuth + email whitelist (`ALLOWED_EMAILS`)
- Session via Cookie / Bearer JWT — no credentials in URL query strings

## 📁 Project Structure

```
tradingview/
├── backend/
│   ├── src/
│   │   ├── main.rs           # Axum server bootstrap
│   │   └── tradingview.rs    # UDF API + Tencent Finance proxy
│   └── .env.example
└── frontend/
    ├── index.html            # Main chart app
    ├── login.html
    ├── auth-config.js
    └── charting_library/
```

## 🛠️ Tech Stack

| Layer | Technology |
|-------|-----------|
| **Backend** | Rust, Axum, reqwest, tokio, encoding_rs |
| **Data Source** | Tencent Finance public HTTP APIs |
| **Frontend** | TradingView Charting Library, Vanilla JS |
| **Auth** | Google Identity Services (OAuth) |

## 🚀 Quick Start

### Backend

```bash
cd backend
cp .env.example .env
# Edit .env: AUTH_DISABLED, ALLOWED_EMAILS, TRACKED_SYMBOL
cargo run
```

Server listens on `http://0.0.0.0:3000`.

**`.env` example:**

```env
RUST_LOG=info
AUTH_DISABLED=true
ALLOWED_EMAILS=your@gmail.com
TRACKED_SYMBOL=sh600519,sz000001,sz300750,sh601318
```

### Frontend

```bash
cd frontend
python serve.py
# Open http://localhost:8080
```

Local dev: `auth-config.js` points API to `http://localhost:3000`.

## 📡 API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /config` | UDF configuration (auth required) |
| `GET /symbols?symbol=sh600519` | Symbol metadata |
| `GET /search?query=600519` | Symbol search |
| `GET /history?symbol=...&resolution=...&from=...&to=...` | K-line history |
| `GET /quotes?symbols=sh600519,sz000001` | Latest quotes for watchlist |
| `GET /auth/verify?email=...` | Whitelist check |

## 📜 License

MIT
