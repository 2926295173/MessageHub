<p align="center">
  <img src="docs/logo/phonebridge-icon-1024.png" alt="PhoneBridge" width="128" />
</p>

<h1 align="center">PhoneBridge</h1>

<p align="center">
  <em>局域网优先、自托管、跨平台的桥接系统 —— 在一台 message-center 上统一管理多部 Android 手机。</em>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh.md">简体中文</a>
</p>

**状态：** 🚧 预发布（Pre-alpha），仍在积极开发。MVP 范围：设备配对、通知同步、短信收发、通话控制。里程碑：M0 ✅ 脚手架 · M1 ✅ message-center 内核 · M2 ✅ 发现 / 配对 / WebSocket · M3 ✅ 业务通道 · M4 ✅ CI / OpenAPI / 实时推送 · M5 ✅ Android 客户端 · **M6 加固** ✅（Android Keystore 长期身份、滑动撤销反向通道、健壮的 `SmsReceiver`、持久化 WebSocket）。

## 项目概览

PhoneBridge 是一个由三部分组成的系统：

- **Android Agent**（Kotlin + Jetpack Compose，包名 `im.zyx.phonebridge`）：通过 mDNS 在局域网内注册、维持前台服务，并通过 TLS+WebSocket 连接把通知 / 短信 / 通话状态转发给 message-center。
- **Message Center**（Rust，二进制 `message-center`）：中央 broker。单一二进制，无原生 GUI。承载本地 Web 控制台、WebSocket 端点与 mDNS 应答。它把 Android 事件分发到两个表面：Web 控制台与桌面通知端点。
- **Desktop Notifier**（Rust，二进制 `phonebridge-display`）：订阅 message-center 上的 `/ws/display`，并通过宿主系统通知 API 呈现手机事件（Linux：`org.freedesktop.Notifications`；macOS / Windows：规划中）。

不上云。无遥测。无账号。在本地网络下完全离线即可工作。

灵感来自 KDE Connect 与 Microsoft Phone Link；明确聚焦于稳定的通知 + 短信同步。

## 架构

```
                                       ┌──────────────┐
                                       │  Desktop     │
                                       │  Notifier    │
                                       │ (Rust binary)│
                                       └──────▲───────┘
                                              │ /ws/display
                                              │
┌──────────┐    TLS+WS    ┌────────────────┐   │   ┌──────────┐
│ Android  │ ◀─────────▶  │ Message Center │   │   │ Browser  │
│ Agent    │              │ (Rust binary)  │ ◀─┴─▶│          │
│ (Kotlin) │              │   - SQLite     │       │          │
└──────────┘              │   - mDNS       │       └──────────┘
                          │   - Web console│
                          └────────────────┘
```

三部分通信方式如下：

- **Android Agent ↔ Message Center**：基于 TLS+WebSocket 的全双工 `Envelope` 帧通信。Agent 始终是 WebSocket 客户端。
- **Desktop Notifier → Message Center**：订阅 `/ws/display?token=<hex>`；接收手机事件，并回送 `quick-reply` / `mark-read` / `dismiss` 等动作。
- **Browser → Message Center**：标准 HTTP 提供 REST 接口，加 WebSocket 提供实时事件推送（`/ws/console`）。

## 仓库目录

```
MessageHub/                         # 上游仓库：github.com/2926295173/MessageHub
├── crates/                         # Rust workspace
│   ├── phonebridge-proto/          # 协议数据类型（由 JSON Schema 驱动）
│   ├── phonebridge-core/           # 配置、路径、日志、错误
│   ├── phonebridge-crypto/         # ECDH P-256、HKDF、自签名证书
│   ├── phonebridge-net/            # mDNS + WebSocket 处理
│   ├── phonebridge-storage/        # sqlx 迁移与模型
│   ├── phonebridge-bus/            # 进程内事件总线（为插件钩子预留）
│   ├── message-center/             # 主二进制（中央 broker）
│   └── phonebridge-display/        # 桌面通知端（订阅 /ws/display）
├── frontend/                       # Next.js 16（App Router，静态导出）
├── android/                        # Kotlin + Compose 客户端（im.zyx.phonebridge）
├── schema/                         # protocol.schema.json（协议真源）
├── docs/                           # 协议、威胁模型、权限、开发设置
└── scripts/                        # setup.sh、dev-run.sh、e2e-smoke.sh
```

## MVP 范围

- **Android：** 设备注册、局域网发现（mDNS）、配对（4 位数字码 + ECDH）、通知监听、短信收发、通话状态监测、应答 / 挂断。
- **桌面：** 设备管理、WebSocket 连接管理、通知中心、短信中心、通话控制、配对管理、内嵌 Web 控制台。

**范围之外**（架构上需为这些预留空间，但暂不实现）：插件系统、ADB 控制、AI 自动分类、自动化规则、Webhook、Telegram Bot、Home Assistant、多用户、远程网关。

## 更新日志

重要变更记录在 [`CHANGELOG.md`](CHANGELOG.md)。过去的条目包括：`phonebridge-daemon` → `message-center` 二进制改名（BREAKING —— 详见 CHANGELOG 内的迁移表，涉及 systemd unit / `RUST_LOG` / 安装路径）、`--no-tls` 模式下默认绑定端口从 `8443` 改为 `8080`。

## 快速开始（开发）

前置条件与各组件的构建说明见 [`docs/dev-setup.md`](docs/dev-setup.md)。

```bash
# 1. 准备配置目录
bash scripts/setup.sh

# 2. 构建并运行 message-center（前台）
cargo run -p message-center

# 3. 启动 Web 控制台（另开终端，dev 模式带热重载）
cd frontend
npm install
npm run dev   # http://localhost:3000/console

# 4. 安装 Android 客户端（需要连接的设备或模拟器）
cd ../android
./gradlew :app:installDebug
```

## 安全

Android Agent 与 Message Center 之间的所有跨组件流量均使用 TLS；设备身份绑定到配对时通过 ECDH 派生的长期证书并予以 pin。详见 [`docs/threat-model.md`](docs/threat-model.md)。

## 许可证

PhoneBridge 采用
[**GNU Affero 通用公共许可证 v3.0 或更新版本**](LICENSE)
（`AGPL-3.0-or-later`）许可。完整文本在 [`LICENSE`](LICENSE)。

您可以出于任何目的学习、修改、再分发本源码（源码或二进制形式），**条件是您分发的任何修改版本 —— 包括在网络服务器上运行的修改版本 —— 也必须在同样的 AGPL-3.0 条款下以完整的对应源代码形式提供**（参见 AGPL-3.0 第 13 条）。

Rust workspace 的 SPDX 标识符：`AGPL-3.0-or-later`（见 [`Cargo.toml`](Cargo.toml) 中的 `license` 字段）。
