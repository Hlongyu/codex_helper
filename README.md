# XXSwitch

本地 Codex 网关管理器，使用 Rust + Tauri + React 实现。

它的目标是把 Codex 的模型请求接到本机路由，再由本工具转发到你配置的上游供应商。开启接管时只修改 `~/.codex/config.toml` 中两个字段：

```toml
[model_providers.custom]
base_url = "http://127.0.0.1:18080/v1"
experimental_bearer_token = "xxswitch-local-token"
```

除此之外，`config.toml` 里的其他内容都保持原样，不再做基础模板、全量 merge、供应商配置覆盖等逻辑。

## 使用方式

使用前请先通过 Codex 官方方式完成登录，并确保本机已有可用的官方登录态。

然后在 XXSwitch 中：

1. 添加上游供应商，填写 Base URL 与 API Key。
2. 启用需要参与路由的供应商。
3. 打开右上角“接管 Codex”开关。
4. Codex 会连接本机 `/v1`，真实请求由 XXSwitch 转发到上游供应商。

## 功能

- 本地 `/v1/*` 路由转发。
- 供应商管理：Base URL、API Key、启用状态。
- 余额查询：支持 Sub2API 与 New API。
- 使用统计：读取本机 Codex 会话目录中的 token 统计元数据。
- 接管 Codex：只补丁修改 `base_url` 与 `experimental_bearer_token`。
- UI 设计稿保存在 `docs/design/`。

## 本地路由

本地路由只代理 OpenAI-compatible 的 `/v1/*` 请求。请求进入本地路由后，工具会校验本地访问令牌，并把上游请求的 `Authorization` 替换成当前可用供应商的 API Key。

本地路由不会代理 Codex 官方登录、账号、授权或会话请求，也不会复用官方 token/cookie/session。

## 额度查询

Sub2API 使用：

```text
GET /v1/usage
Authorization: Bearer sk-...
```

New API 支持：

- 普通 `sk-` 密钥查询令牌额度：`GET /api/usage/token/`
- 用户访问令牌查询账户余额：`GET /api/user/self`，需要填写用户 ID

为了避免泄露密钥，不支持把 Key 放进 URL 查询参数。

## 配置位置

Codex 配置：

```text
~/.codex/config.toml
```

XXSwitch 状态：

```text
~/.codex/config-manager/state.json
```

## 开发

安装依赖：

```bash
pnpm install
```

启动 Tauri 桌面开发模式：

```bash
pnpm tauri dev
```

只启动前端开发服务器：

```bash
pnpm dev
```

检查：

```bash
pnpm build
cd src-tauri
cargo check
```

打包：

```bash
pnpm tauri build
```
