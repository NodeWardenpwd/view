# 📊 View — A-Share Chart Platform v3.0

[🇨🇳 中文版](README_CN.md) · [Main README](README.md)

> **v3.0 Architecture Upgrade**
>
> The project has been fully decentralized: **AKTools dependency removed**. It now runs on a **pure Rust backend** powered by **East Money public APIs**, with native support for intraday candles (1m/5m/15m/30m/60m), high-frequency polling pseudo-realtime quotes, and browser-disguised HTTP requests that bypass overseas IP anti-bot restrictions.

A self-hosted **A-share (China stock)** charting platform. A lightweight Rust Axum backend proxies East Money market data into the TradingView UDF protocol, paired with a professional Charting Library frontend.

## ✨ Features

### Market Data (v3.0)
- **East Money K-line API** — Daily / weekly / monthly + intraday (1/5/15/30/60 min), forward-adjusted (qfq)
- **No local database required** — On-demand proxy; no historical sync pipeline
- **Anti-bot headers + retry** — Browser User-Agent / Referer disguise with 3-attempt retry for overseas deployments
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
│   │   └── tradingview.rs    # UDF API + East Money proxy
│   └── .env.example
└── frontend/
    ├── index.html
    ├── login.html
    ├── auth-config.js
    └── charting_library/
```

## 🛠️ Tech Stack

| Layer | Technology |
|-------|-----------|
| **Backend** | Rust, Axum, reqwest, tokio |
| **Data Source** | East Money public HTTP APIs |
| **Frontend** | TradingView Charting Library, Vanilla JS |
| **Auth** | Google Identity Services (OAuth) |

## 🚀 Quick Start

### Backend

```bash
cd backend
cp .env.example .env
cargo run
```

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
```

## 📡 API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /config` | UDF configuration |
| `GET /symbols?symbol=sh600519` | Symbol metadata |
| `GET /search?query=600519` | Symbol search |
| `GET /history?symbol=...&resolution=...&from=...&to=...` | K-line history |
| `GET /quotes?symbols=sh600519,sz000001` | Latest quotes |
| `GET /auth/verify?email=...` | Whitelist check |

## 📜 License

MIT
