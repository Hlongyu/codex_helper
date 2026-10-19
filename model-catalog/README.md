# XXSwitch 模型能力发布目录

维护 `catalog.json` 后提升 `version`。`openai` 和 `deepseek` 都是完整 Codex 模型条目数组；`default` 是未匹配模型使用的完整条目。所有 slug 必须唯一。应用只修改展示 ID、显示名、排序与可见性，其他能力直接复制。

初始数据从本项目已有 OpenAI / DeepSeek 能力快照导出，不代表重新验证过的上游最新版。后续由维护者直接更新此文件。

本地运行：在仓库根目录执行 `npm run catalog:serve`。接口 `GET /catalog.json` 支持查询参数，不重定向，不要求认证。部署 Cloudflare 静态托管时发布此目录即可。
