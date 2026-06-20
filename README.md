# Codex Helper

一个本地 Codex 配置管理器，使用 Rust + Tauri + React 实现。

它用于管理 `~/.codex/config.toml`，重点是通过增量合并配置，而不是直接覆盖整个文件。这样 Codex 后续新增的配置项仍然可以保留，应用模板时只写入需要管理的字段。

## 使用方式

使用前请先通过 Codex 官方方式完成登录，并确保本机已有可用的官方登录态。

这个工具的目标不是替代 Codex 登录流程，而是在保留官方登录态的前提下，通过管理 `config.toml` 选择是否使用自定义 API 供应商访问。典型场景是继续使用官方 Codex App，同时把模型访问切换到配置好的 `model_providers.custom`。

左侧切换供应商只会切换当前编辑和预览对象。真正写入 `~/.codex/config.toml` 并让 Codex 使用该供应商，需要点击右上角的“应用”。

## 功能

- 管理基础配置模板。
- 管理多个供应商配置。
- 快速编辑 `model_providers.custom` 下的 `base_url` 与 `experimental_bearer_token`。
- Token 默认隐藏，可按需显示。
- 查看当前实际 `config.toml`。
- 预览选择供应商后最终会写入磁盘的 TOML。
- 应用配置前自动计算差异。
- 应用时备份原始 `config.toml`。
- 标记已经应用的供应商，和当前选中的供应商分开显示。
- 查询当前选中供应商的额度，支持 Sub2API 和 New API。
- 统计本机 Codex 使用情况，按时间、供应商、模型筛选 token 明细和金额。

## 供应商配置

供应商配置使用 TOML 编辑，常用字段会在界面中提供快捷输入：

```toml
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.com/v1"
experimental_bearer_token = "sk-..."
```

基础模板只用于放通用配置。供应商配置会在基础模板之上增量合并，应用时只改动本工具管理到的字段。

## 额度查询

额度查询只针对当前选中的供应商，密钥默认使用该供应商配置中的 `experimental_bearer_token`，也可以单独填写查询令牌。

Sub2API 使用：

```text
GET /v1/usage
Authorization: Bearer sk-...
```

New API 支持两种模式：

- 普通 `sk-` 密钥查询令牌额度：`GET /api/usage/token/`
- 用户访问令牌查询账户余额：`GET /api/user/self`，需要填写用户 ID

为了避免泄露密钥，不支持把 Key 放进 URL 查询参数。

## 使用统计

使用统计只读取本机 Codex 会话目录中的 token 统计元数据，不读取 prompt 或 response 内容。

统计入口是左下角“使用统计”。首次进入会扫描 `~/.codex/sessions`，之后切换筛选条件会复用内存缓存；点击“刷新统计”才会重新扫描磁盘。

明细会区分：

- 输入（未缓存）
- 输入（已缓存）
- 输出
- 推理输出
- 金额

金额按内置 GPT 官方 API 价格规则计算，鼠标悬停可以查看计算详情；无法匹配价格的模型按 0 处理。

## 配置位置

应用会读取并写入：

```text
~/.codex/config.toml
```

应用自身的状态文件保存在：

```text
~/.codex/config-manager/state.json
```

其中 `config.toml` 里的管理标记只是注释标记。即使 Codex 重写配置时移除了该注释，已应用供应商状态仍以 `state.json` 为准。

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

前端构建：

```bash
pnpm build
```

Rust 后端检查：

```bash
cd src-tauri
cargo check
```

## 打包

```bash
pnpm tauri build
```

当前 Tauri 配置默认生成 Windows NSIS 安装包。

## GitHub Actions

项目包含两个 workflow：

- `CI`：在 push、pull request 或手动触发时运行前端构建和 Rust 检查。
- `Build Desktop App`：在推送 `v*` 标签或手动触发时构建 Windows 桌面安装包，并上传构建产物。
