# 更新记录

## v1.2.18 - 2026-08-05

### CI/CD

- Windows 打包工具改为缓存到项目的 `target/.tauri`，兼容以 System 用户运行的 GitLab Shell Runner，避免 NSIS `makensis` 无法启动。
- macOS 发布作业直接校验 Tauri 最终生成并签名的 DMG，不再依赖打包后会被清理的临时 `.app` 目录。

## v1.2.17 - 2026-08-05

### CI/CD

- 增加 GitLab CI 发布流程，推送 `v*` 标签时同时构建 Windows NSIS 与 macOS Universal DMG，并创建 GitLab Release。
- GitLab macOS 构建固定使用带 Node、Rust、Xcode 与 Developer ID Application 签名身份的 M1 Shell Runner。
- macOS Shell 作业通过资源组串行执行，避免共享工作区并发导致 Git shallow metadata 冲突。
- Linux 容器作业固定使用 Docker Autoscaler Runner，避免带 `docker` 标签的 Shell Runner 忽略容器镜像。

### 发布

- GitHub Release 与 GitLab Release 统一从本文件提取对应版本的更新说明。
- 本地 `origin` 已配置同时推送 GitHub 与 GitLab，分支和新版本标签可一次同步到两个平台。

## v1.2.16 - 2026-08-05

### 新增

- 供应商自动禁用恢复策略支持按供应商选择“每天”“每周”“每月”或“不刷新”，Codex 与 Claude 供应商均可独立配置。
- 每天刷新可设置具体时间；每周刷新可设置星期与时间；每月刷新可设置日期与时间，短月份会自动使用当月最后一天。
- 选择“不刷新”后，供应商因连续失败进入自动禁用状态时不会再自动启用，需手动恢复。

### 兼容与行为

- 现有供应商默认保持每天 `00:00` 恢复，与升级前行为一致。
- 修改已自动禁用供应商的刷新策略时，会根据最后一次失败时间重新计算当前额度周期，避免意外提前恢复。

## v1.2.15 - 2026-08-02

### 新增

- Codex 自定义 provider 接管时注入 `x-openai-actor-authorization = "local-image-extension"`，支持使用 Codex 内置 `image_gen` 工具。
- 根据本机是否已配置 ChatGPT 登录态自动设置 `requires_openai_auth`；只检查配置，不校验凭据是否过期，也不触发刷新或联网验证。
- 增加“强制不使用 OpenAI 登录态”开关，可将 `requires_openai_auth` 固定为 `false`。关闭接管时会恢复原有字段和 actor header。
- 增加 Responses API Debug 模式，可记录脱敏后的入站请求、实际上游请求与上游原始响应，并在请求详情中查看。
- 支持跨分页选择 Debug 快照并导出 JSON，便于比较供应商请求转换、响应内容和缓存差异。

### 安全与限制

- Debug 快照中的认证头、API Key 和 Cookie 会脱敏；每段请求体或响应体最多保存 4 MiB，超出部分会标记为截断。
- Debug 快照仍可能包含完整提示词和模型输出，保存在 `~/.codex/config-manager/debug-captures/`，应按敏感数据保护。
- actor authorization header 是 Codex 对自定义 provider 的能力标记，不是 ChatGPT 凭证；请求仍通过当前 XXSwitch provider 转发。
