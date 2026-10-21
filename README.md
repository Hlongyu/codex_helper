# XXSwitch

XXSwitch 是一个使用 Rust、Tauri 和 React 构建的本地 AI 请求路由器。它为 Codex、Claude Code 和 Pi 提供统一的供应商管理、模型路由、调用记录与 Skills 管理界面。

XXSwitch 只记录实际经过本地路由的生成调用及其 Token、延迟和结果，不进行价格维护、金额估算或费用统计。

## 核心功能

- **多客户端接管**：支持 Codex、Claude Code 和 Pi，各客户端可独立启用。
- **供应商管理**：分别管理 OpenAI-compatible 与 Claude-compatible 上游，支持启用、停用和拖拽排序。
- **模型配置**：从上游拉取模型，选择参与路由的模型，并配置客户端模型到上游模型的映射。
- **协议兼容**：Codex 供应商可选择 Responses API 或 Chat Completions 兼容模式。
- **Fast 模式**：Codex 供应商可强制使用 `service_tier: "priority"`。
- **上下文接力**：可启用 Codex 实验性的 Token Budget，并使用内置 Notes/History 在上下文窗口之间接续任务。
- **真实模型测速**：选择实际模型和同步或流式请求，使用自定义内容发起生成请求，并查看延迟与完整回复。
- **故障切换**：按供应商顺序路由；连续三次供应商故障后自动停用，并在下一个本地自然日恢复。
- **系统代理**：上游请求、模型测试、余额查询和应用更新自动使用 Windows 或 macOS 的系统 HTTP/HTTPS 代理，并兼容代理环境变量。
- **余额查询**：支持 Sub2API 与 New API，余额查询与模型路由相互独立。
- **调用观测**：筛选和导出真实路由调用，并查看单次请求的模型映射、Token 明细、分段耗时、上游链路和错误诊断。
- **Responses Debug 快照**：按需记录入站请求、实际上游请求和上游原始响应，用于比较供应商可见载荷与缓存差异。
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

XXSwitch 将 `model_provider` 指向本地 `custom` provider，维护其 Base URL、本地访问令牌和名称，并根据设置同步 `features.remote_compaction_v2`。接管期间会合并设置 `http_headers = { "x-openai-actor-authorization" = "local-image-extension" }`，让当前 `custom` provider 暴露 Codex 内置 `image_gen`。同时，XXSwitch 只判断是否已配置 ChatGPT 登录态：已配置时设置 `requires_openai_auth = true`，未配置时设置为 `false`；不会检查凭据是否过期，也不会主动刷新或联网验证。用户可通过“强制不使用 OpenAI 登录态”将该字段固定为 `false`。该 header 只是自定义 provider 的能力标记，不是 ChatGPT 凭证；接管请求仍使用本地访问令牌并转发到当前供应商。关闭接管后会恢复这些字段的原值。

启用“实验性上下文接力”时，XXSwitch 同步设置 `features.token_budget.enabled` 和 `features.token_budget.use_history_notes_extension`，具体提醒阈值、兜底缓冲和提示模板继续使用当前模型目录提供的默认值。该功能与远程压缩独立配置。

### 供应商融合模式

在 Codex 或 Claude 供应商的编辑页开启「融合模式」，并在模型配置中选定要发布的模型。客户端模型列表将显示「模型名（供应商名）」。Codex 需要彻底退出后重新打开以加载更新的目录。

融合模型使用供应商固定 ID 路由：只发往对应供应商，不参与跨供应商故障切换；普通、不带供应商的模型请求会排除所有开启融合模式的供应商。关闭融合、停用或删除供应商后，旧融合模型不会回落到其他线路。供应商改名只影响显示名称。

模型能力与默认模板统一来自维护者发布的目录，程序只按模型 ID 筛选并添加供应商标识。未收录模型使用目录中的默认模板，不在程序中推导能力。现有模型映射仍在发送上游之前生效。

### 失败自动禁用

全局默认在供应商连续失败 3 次后自动禁用，并按该供应商的刷新周期恢复。可以在“设置 → 本地代理”统一关闭自动禁用或修改连续失败次数。

每个 Codex、Claude 供应商可选择继承全局策略，或单独设置是否启用及失败次数。只统计未手动停用的供应商；关闭自动禁用后会清除累计失败次数，并立即恢复因此前策略而停用的供应商。手动停用不受此设置影响。

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

## 从 new-api 一键导入供应商

支持 `xxswitch://v1/import` 链接导入 Codex 和 Claude 供应商。点击网页上的“导出到 XXSwitch”后，应用会显示配置摘要和脱敏后的 API Key，确认后新增到供应商列表。默认以停用状态保存，可在确认窗口勾选启用；已有供应商不会被覆盖。

站点只需按协议生成包含名称、Base URL、API Key 和接口格式的链接，无需新增 ticket 后端接口。字段约定、按钮代码和安装验证步骤见 [new-api 接入指南](docs/integrations/new-api-provider-import.md)。

## 调用记录

XXSwitch 仅记录经过本地路由的生成调用，模型列表等非生成请求不会进入调用统计。记录内容包括：

- 请求时间、请求路径、客户端模型、上游模型与最终供应商
- 输入、非缓存输入、缓存输入、输出、推理输出和总 Token
- 发起上游前、等待响应头、响应头至首字节、首字节至结束等分段耗时
- 成功、失败或取消状态，以及路由尝试次数和供应商链路
- 本地与上游请求 ID、远程压缩审计结果和错误详情

请求列表支持按状态、供应商、模型和日期筛选，可搜索请求 ID 或错误信息、实时刷新并导出当前页 CSV。点击任意记录可打开请求详情，旧记录未采集的分段耗时会明确标记为不可用。

在“本地代理”中开启 Debug 模式后，新的 `POST /v1/responses` 调用会在请求详情中增加
“入站请求 / 上游请求 / 上游响应”快照。认证头、API Key 和 Cookie 始终脱敏；每段请求体或
响应体最多保存 4 MiB，超过部分会标记为截断。快照反映 XXSwitch 收到和发出的线上数据，
不能观察供应商服务端未回传的隐藏提示词或内部处理。

请求列表支持跨分页勾选多条 Debug 快照并导出为一个 JSON 文件。导出包包含对应的路由日志、
完整快照和读取失败清单，可直接交给 Codex 比较不同供应商的请求转换、响应与缓存差异。

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
`http://host:6789/ai-gate/api/me/upstreams/usage`。首次使用“聚合全部账号”测试查询后，
可在账号下拉框中选择具体上游账号，也可以继续聚合全部账号。多账号余额仅在单位相同时汇总；
不同单位会分别显示。

为避免泄露密钥，不支持把 Key 放进 URL 查询参数。

## 数据位置

XXSwitch 的状态、路由日志、供应商故障记录和 Skill Library 位于：

```text
~/.codex/config-manager/
```

Debug 快照单独保存在 `~/.codex/config-manager/debug-captures/`，关闭 Debug 模式不会删除
已有快照。快照包含完整提示词和模型输出，应按敏感数据保护该目录。

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

## 模型能力目录服务

正式更新地址为 `https://models.xxcd.top/catalog.json`。

本机开发时运行 `npm run catalog:serve`，在 `http://127.0.0.1:18081/catalog.json` 提供静态 JSON（仅监听本机）。维护文件是 `model-catalog/catalog.json`，修改后下次请求立即生效，无需重启服务。

在 XXSwitch「设置 → 应用 → 模型能力目录」点击「更新模型能力」。程序不携带供应商凭据，下载并校验完整目录后保存至 `~/.codex/config-manager/capability-catalog.json`，然后按供应商选定模型重新生成 `router-models.json`。失败保留原缓存；首次离线使用同一目录随安装包附带的快照。目录更新后需彻底重启 Codex。

目录仅包含 `schema_version`、发布 `version`、`openai` 模型数组、`deepseek` 模型数组和 `default` 完整模型条目。上下文、基础指令、思考档位、工具等字段由维护者直接维护；用户没有自定义目录入口。当前初始目录来自项目已有能力快照，未重新抓取上游最新版。显式开启的 GPT-5.6 及以上长上下文设置（按主、次版本号识别，例如 5.10、6、6.1）优先于远端目录，普通和融合模型均生效；未开启时沿用目录参数。

正式服务地址固定在 `src-tauri/src/model_catalog.rs` 的 `URL` 常量中。远端下载测试可运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib remote_capability_service_round_trip -- --ignored`，使用临时目录验证缓存和供应商合并。

本机端到端测试：先启动目录服务，再运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib local_capability_service_round_trip -- --ignored`。测试在临时目录保存缓存，不修改真实用户配置。

请求体始终在本地解压读取路由信息。「以未压缩 JSON 转发」关闭时，未修改的 zstd 请求原字节透传；融合 ID、模型映射、协议或参数修改后重新压缩，发送正确的 Content-Encoding，并重新计算长度。开启时发送解压后的 JSON。
