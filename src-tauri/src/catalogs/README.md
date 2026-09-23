# Codex 模型目录

- `codex-models.json`：CLI `0.153.4` 的 bundled 目录，由 `pnpm catalog:update` 更新。
- `codex-models-gpt6.json`：2026-09-23 使用官方 CLI `0.156.0` 的 `debug models` 刷新后，提取的 `gpt-6-sol`、`gpt-6-luna` 原始 capability 条目，包含各自完整提示词。该版本的 `debug models --bundled` 尚未包含这两个模型。

运行时先选择合格的外部 bundled 目录或内置目录，再补充缺少的 Sol/Luna 条目；同名外部条目优先。这里的补充只提供模型能力，实际可路由模型仍取决于服务商配置。

更新补充目录时，使用已登录官方账号、未被接管的 Codex 配置运行 `codex debug models > refreshed-models.json`，确认结果包含两个模型，然后提取原始条目。不要使用接管生成的 `model_catalog_json` 作为官方来源，也不要把 Astra 或 GPT-5.6 的提示词改名后代替新模型模板。

API 参数兼容依据：

- [GPT-6 参数迁移](https://developers.openai.com/api/docs/guides/latest-model#gpt-6-astra-update-api-and-model-parameters)
- [GPT-6 Sol](https://developers.openai.com/api/docs/models/gpt-6-sol)
- [GPT-6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna)

注意区分 API 的能力上限和 Codex capability 默认值：当前官方 Codex 模板默认上下文为 272,000、最大上下文为 872,000；Sol 的 Codex 推理选项包含 Ultra，Luna 到 Max。原始 API 支持 `none`，路由层保留该值；启用推理的工具请求需要 Responses，`none` 的函数调用可以使用 Chat Completions。
