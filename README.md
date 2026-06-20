# Codex Helper

一个本地 Codex 配置管理器，使用 Rust + Tauri + React 实现。

它用于管理 `~/.codex/config.toml`，重点是通过增量合并配置，而不是直接覆盖整个文件。这样 Codex 后续新增的配置项仍然可以保留，应用模板时只写入需要管理的字段。

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

## 配置位置

应用会读取并写入：

```text
~/.codex/config.toml
```

应用自身的状态文件保存在：

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

## 目标仓库

```text
git@github.com:Hlongyu/codex_helper.git
```
