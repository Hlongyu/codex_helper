# XXSwitch 模型能力发布目录

维护 `catalog.json` 后提升 `version`。`openai` 和 `deepseek` 都是完整 Codex 模型条目数组；`default` 是未匹配模型使用的完整条目。所有 slug 必须唯一。应用只修改展示 ID、显示名、排序与可见性，其他能力直接复制。

初始数据从本项目已有 OpenAI / DeepSeek 能力快照导出，不代表重新验证过的上游最新版。后续由维护者直接更新此文件。

同步官方 OpenAI 模板：`pnpm catalog:update --version YYYY-MM-DD.N`。脚本通过 GitHub API 获取 [OpenAI Codex models.json](https://github.com/openai/codex/blob/main/codex-rs/models-manager/models.json)，更新当前实际发布和内置的 `model-catalog/catalog.json`。使用 `--model gpt-6.1-sol` 可只更新指定模型（可重复指定）；使用 `--source /path/to/models.json` 可导入已下载的官方文件。同步保留上游已移除的旧模型、DeepSeek 和默认模板，校验失败不写文件。运行 `pnpm catalog:test` 检查同步逻辑。

官方条目原样保留。若新版上游只有 `model_messages.instructions_template` 而没有 `base_instructions`，额外将该模型自己的模板原文复制到 `base_instructions`，兼容现有客户端的目录校验；不借用其他模型提示词。

`2026-10-20.1` 新增 `gpt-6.1-sol`，来源为上述官方文件（Git blob `72a78e0eacf94103f4babda2a5245aba4ce848c1`），其余模型保持原快照。

`2026-10-27.1` 新增 `gpt-6.1-terra`，来源为上述官方文件（Git blob `489ebe3f2483348641a16160ce8491dd077eabca`）。保留官方默认上下文 500,000、最大可配置上下文 872,000，以及 `low` 至 `ultra` 的推理档位；其余模型保持原快照。

修改本仓库不会自动发布正式服务。发布此目录后，已有用户在「设置 → 应用 → 模型能力目录」更新模型能力并彻底重启 Codex；已有缓存优先于安装包快照。

本地运行：在仓库根目录执行 `npm run catalog:serve`。接口 `GET /catalog.json` 支持查询参数，不重定向，不要求认证。部署 Cloudflare 静态托管时发布此目录即可。
