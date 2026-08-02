# 更新记录

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
