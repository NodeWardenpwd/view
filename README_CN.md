# 📊 View — A 股看盘平台 v3.0

[🇺🇸 English README](README_EN.md) · [Main README](README.md)

> **v3.0 架构升级**
>
> 项目已升级至 **v3.0 版本**，架构全面去中心化，**移除 AKTools 依赖**。升级为 **纯净的 Rust 后端 + 东方财富公开高速 API** 驱动，完美支持分钟线（1m/5m/15m/30m/60m）及高频轮询伪实时行情，并通过浏览器伪装请求头与重试机制，有效穿透海外机房 IP 的风控限制。

自托管 **A 股** 看盘系统：Rust Axum 后端将东方财富行情动态代理为 TradingView UDF 协议，前端使用专业 Charting Library 渲染 K 线。

## ✨ 功能特性

### 行情数据（v3.0）
- **东财 K 线接口** — 日/周/月 + 分钟线（1/5/15/30/60），前复权
- **无需本地数据库** — 按需动态拉取，无需 TimescaleDB 全量同步
- **风控对抗** — 注入浏览器 User-Agent / Referer 等请求头，失败自动重试 3 次
- **轮询伪实时** — 图表与自选股每 10–30 秒轮询最新报价

### 图表
- TradingView 图表库，简体中文界面（`locale: "zh"`）
- 自定义指标适配器骨架（可扩展 Pine Script → JS）
- 多图布局、图表保存/加载、深色/浅色主题

### 安全
- Google OAuth + 邮箱白名单（`ALLOWED_EMAILS`）
- Cookie / Bearer JWT 会话鉴权，URL 不携带敏感参数

## 📁 项目结构

```
tradingview/
├── backend/
│   ├── src/
│   │   ├── main.rs           # Axum 服务入口
│   │   └── tradingview.rs    # UDF API + 东财代理
│   └── .env.example
└── frontend/
    ├── index.html            # 主看盘界面
    ├── login.html
    ├── auth-config.js
    └── charting_library/
```

## 🛠️ 技术栈

| 层级 | 技术 |
|------|------|
| **后端** | Rust, Axum, reqwest, tokio |
| **数据源** | 东方财富公开 HTTP API |
| **前端** | TradingView Charting Library, Vanilla JS |
| **认证** | Google Identity Services (OAuth) |

## 🚀 快速启动

### 后端

```bash
cd backend
cp .env.example .env
# 编辑 .env：AUTH_DISABLED、ALLOWED_EMAILS、TRACKED_SYMBOL
cargo run
```

服务监听 `http://0.0.0.0:3000`。

**`.env` 示例：**

```env
RUST_LOG=info
AUTH_DISABLED=true
ALLOWED_EMAILS=your@gmail.com
TRACKED_SYMBOL=sh600519,sz000001,sz300750,sh601318
```

### 前端

```bash
cd frontend
python serve.py
# 浏览器访问 http://localhost:8080
```

本地开发时 `auth-config.js` 会将 API 指向 `http://localhost:3000`。

## 📡 API 接口

| 接口 | 说明 |
|------|------|
| `GET /config` | UDF 配置（需鉴权） |
| `GET /symbols?symbol=sh600519` | 标的元数据 |
| `GET /search?query=600519` | 代码搜索 |
| `GET /history?symbol=...&resolution=...&from=...&to=...` | 历史 K 线 |
| `GET /quotes?symbols=sh600519,sz000001` | 自选股最新价 |
| `GET /auth/verify?email=...` | 白名单校验 |

## 📜 许可证

MIT
