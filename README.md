# XXSwitch

XXSwitch 是一个使用 Rust、Tauri 和 React 构建的本地 AI 请求路由器。它为 Codex、Claude Code 和 Pi 提供统一的供应商管理、模型路由、调用记录与 Skills 管理界面。

XXSwitch 只记录实际经过本地路由的生成调用及其 Token、延迟和结果，不进行价格维护、金额估算或费用统计。

## 核心功能

- **多客户端接管**：支持 Codex、Claude Code 和 Pi，各客户端可独立启用。
- **供应商管理**：分别管理 OpenAI-compatible 与 Claude-compatible 上游，支持启用、停用和拖拽排序。
- **模型配置**：从上游拉取模型，选择参与路由的模型，并配置客户端模型到上游模型的映射。
- **协议兼容**：Codex 供应商可选择 Responses API 或 Chat Completions 兼容模式。
- **Fast 模式**：Codex 供应商可强制使用 `service_tier: "priority"`。
- **真实模型测速**：选择实际模型和同步或流式请求，使用自定义内容发起生成请求，并查看延迟与完整回复。
- **故障切换**：按供应商顺序路由；连续三次供应商故障后自动停用，并在下一个本地自然日恢复。
- **余额查询**：支持 Sub2API 与 New API，余额查询与模型路由相互独立。
- **调用观测**：筛选和导出真实路由调用，并查看单次请求的模型映射、Token 明细、分段耗时、上游链路和错误诊断。
- **Skills 管理**：扫描 Codex、Claude Code 和 Pi 的 Skills，并通过 XXSwitch Skill Library 管理共享与暴露。
- **应用更新**：支持从 GitHub Release 检查并安装新版本。

## 快速开始

1. 添加 Codex 或 Claude 供应商，填写名称、Base URL 和 API Key。
2. 进入“模型配置”，从上游获取模型并选择需要参与路由的模型。
3. 在供应商列表使用“模型测试”，选择模型、请求方式和测试内容验证真实回复。
4. 启用可用供应商，并通过拖拽调整路由优先级。
5. 进入“路由”，开启需要接管的 Codex、Claude Code 或 Pi 客户端。

默认本地路由地址为：

```text
http://127.0.0.1:18080/v1
```

路由使用本地访问令牌校验请求，并在转发时替换为所选供应商的 API Key。XXSwitch 不代理官方登录、账号、授权或会话请求，也不复用官方 token、cookie 或 session。

## 客户端配置

XXSwitch 只维护各客户端中由自己接管的字段，并在关闭接管时恢复保存的值；其他用户配置保持不变。

### Codex

配置文件：`~/.codex/config.toml`

XXSwitch 将 `model_provider` 指向本地 `custom` provider，维护其 Base URL、本地访问令牌和名称，并根据设置同步 `features.remote_compaction_v2`。

### Claude Code

配置文件：`~/.claude/settings.json`

XXSwitch 在 `env` 中维护本地 `ANTHROPIC_BASE_URL`、`ANTHROPIC_API_KEY` 和网关模型发现配置，同时保留无关设置。

### Pi

配置文件：`~/.pi/agent/models.json`

XXSwitch 只维护自己的 `xxswitch` provider 及其模型列表，不覆盖其他 Pi provider。

## 供应商与模型

Codex 供应商的基础配置包含名称、Base URL、API Key、接口协议和 Fast 模式。模型配置与连接参数分开：可从上游获取或手动添加模型，再选择需要参与路由的模型；未限制启用模型时，该供应商兼容任意请求模型。

Claude 供应商使用 Claude-compatible 消息协议，也支持从上游获取模型、启用模型和模型映射。Fast 模式仅适用于 Codex 供应商。

模型测试位于供应商列表外层，不属于基础配置。测试默认内容为 `hi`，可切换同步或流式消息，并显示上游回复结果。

## 调用记录

XXSwitch 仅记录经过本地路由的生成调用，模型列表等非生成请求不会进入调用统计。记录内容包括：

- 请求时间、请求路径、客户端模型、上游模型与最终供应商
- 输入、非缓存输入、缓存输入、输出、推理输出和总 Token
- 发起上游前、等待响应头、响应头至首字节、首字节至结束等分段耗时
- 成功、失败或取消状态，以及路由尝试次数和供应商链路
- 本地与上游请求 ID、远程压缩审计结果和错误详情

请求列表支持按状态、供应商、模型和日期筛选，可搜索请求 ID 或错误信息、实时刷新并导出当前页 CSV。点击任意记录可打开请求详情，旧记录未采集的分段耗时会明确标记为不可用。

“使用统计”由这些路由记录聚合生成，不读取金额、不维护模型价格，也不计算供应商成本。

## 余额查询

Sub2API：

```text
GET /v1/usage
Authorization: Bearer sk-...
```

New API 支持：

- 普通 `sk-` 密钥查询令牌额度：`GET /api/usage/token/`
- 用户访问令牌查询账户余额：`GET /api/user/self`，需要填写用户 ID

AI Gate：

```text
GET /api/me/upstreams/usage
Authorization: Bearer <分配的 Key>
```

查询地址默认使用 Codex 供应商 Base URL 去掉末尾 `/v1` 后的结果，因此当 Base URL 为
`http://host:6789/ai-gate/v1` 时，实际请求为
`http://host:6789/ai-gate/api/me/upstreams/usage`。多账号余额仅在单位相同时汇总；不同单位会分别显示。

为避免泄露密钥，不支持把 Key 放进 URL 查询参数。

## 数据位置

XXSwitch 的状态、路由日志、供应商故障记录和 Skill Library 位于：

```text
~/.codex/config-manager/
```

该目录沿用旧版本路径以兼容已有数据。保存的供应商 API Key 会写入本地状态文件，请按包含敏感信息的配置目录进行保护。

## 开发

环境要求：Node.js 22、pnpm、Rust stable，以及 Tauri 2 对应的系统依赖。

```bash
pnpm install
pnpm tauri dev
```

仅启动前端：

```bash
pnpm dev
```

验证：

```bash
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
```

本地打包：

```bash
pnpm tauri build
```

推送 `v*` 语义化版本标签会触发 GitHub Actions，构建 Windows 与 macOS 安装包并创建 GitHub Release。

## 项目文档

- [领域术语](./CONTEXT.md)
- [文档索引](./docs/README.md)
- [架构决策记录](./docs/adr/)
- [历史界面设计稿](./docs/design/)
