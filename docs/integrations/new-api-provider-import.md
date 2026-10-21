# new-api 一键导入 XXSwitch

XXSwitch 支持从浏览器打开 `xxswitch://v1/import`，预览并确认后新增供应商。此协议参考 CC Switch 的基础字段，但使用独立的 `xxswitch` scheme。它不会注册或接管 `ccswitch://`。

## 链接协议（v1）

```text
xxswitch://v1/import?resource=provider&app=codex&name=My%20New%20API&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-example&apiFormat=openai_responses
```

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `resource` | 是 | 固定为 `provider` |
| `app` | 是 | `codex` 或 `claude`，对应 XXSwitch 的两类供应商 |
| `name` | 是 | 供应商名称，最多 100 个字符 |
| `endpoint` | 是 | 单个 HTTP(S) Base URL；不能带用户名、密码、查询参数、片段或逗号分隔的多个地址 |
| `apiKey` | 是 | 当前用户选择的 API 令牌，最多 4096 字节 |
| `apiFormat` | 否 | `codex` 默认 `openai_responses`，也支持 `openai_chat`；`claude` 只支持 `anthropic` |
| `model` | 否 | 逗号分隔的启用模型列表，最多 100 个；首个同时用于模型测试。省略表示不限制模型 |
| `enabled` | 否 | `true`/`false`，默认 `false`；设置确认窗口中“导入后启用供应商”的初始值，用户可修改 |

参数区分大小写，必须使用 `URLSearchParams` 等标准工具编码，特别是 API Key 中的 `+`、`&`、`=`。链接最多 16 KiB，重复参数会被拒绝。当前版本仅支持上表字段；`config`、`configFormat`、`configUrl`、`usageScript`、`homepage`、`notes` 等额外参数会明确报错，不会静默忽略。它不是 CC Switch 完整协议兼容实现。

## new-api 按钮示例

把按钮放在登录后的令牌列表中，使用该用户有权读取的**完整令牌**，不能使用列表中脱敏后的值。若页面已有读取令牌的接口，复用它，不需要新增 ticket 签发或兑换接口。

```tsx
function buildXXSwitchLink(apiKey: string, name: string, baseUrl: string) {
  const params = new URLSearchParams({
    resource: "provider",
    app: "codex",
    name,
    endpoint: baseUrl, // OpenAI 格式通常为 https://你的域名/v1
    apiKey,
    apiFormat: "openai_responses",
  });
  return `xxswitch://v1/import?${params.toString()}`;
}

// apiKey 应为当前用户选中的完整令牌。
<a href={buildXXSwitchLink(apiKey, "我的 New API", "https://api.example.com/v1")}>
  导出到 XXSwitch
</a>
```

如果需要异步读取完整令牌，先完成读取，再展示可点击的导入链接；不要依赖页面加载后自动跳转，或认为异步请求结束后浏览器仍保留用户点击授权。

Claude 示例把 `app` 改为 `claude`、`apiFormat` 改为 `anthropic`，并传入与上游兼容的 Base URL（通常不带 `/v1`）。模型通过 `model` 传递；如不需要限制可用模型，省略即可。

## 用户体验和保存规则

1. 用户点击导出，浏览器可能询问是否打开 XXSwitch。
2. XXSwitch 启动或从托盘显示，弹出供应商名称、类型、地址、脱敏 Key、协议和模型的确认窗口。
3. 用户选择是否启用并确认，才写入供应商列表。取消不创建供应商，也不会发起上游网络请求。
4. 新供应商追加到列表末尾，不覆盖已有供应商、不调整原有路由优先级、不切换现有选中项、不修改客户端接管开关。启用后可以参与现有路由。
5. 重复点击同一链接并分别确认，会新增多条记录；同一个待处理请求被系统重复投递时会去重。待处理队列最多 16 条，满时忽略后续链接，可处理完后再次点击。

链接及令牌不写入导入日志；确认前仅在后端内存暂存，应用退出后待处理请求消失。预览只向界面返回脱敏后的 Key，确认通过请求 ID 引用原始配置。保存失败时保留请求供重试，成功后消费请求，防止双击重复保存。

**链接包含完整 API Key，URL 编码不是加密。** 不要把生成的链接上传到分析平台、公开分享或记录到服务端访问日志。保存后的 Key 使用 XXSwitch 现有本地配置存储机制。

## 安装与验证

协议注册由 Tauri deep-link 插件和安装包完成，single-instance 插件负责将后续请求交给已有进程。Rust 后端直接处理导入，不增加前端 deep-link 权限，不增加本地 HTTP 监听端口。

- macOS：构建 `.app` 并安装到 `/Applications`，通过系统打开链接验证。仅运行 Vite 或未打包的开发进程无法验证系统协议关联。
- Windows：安装新版本后验证；调试构建会运行时注册当前可执行文件。
- Linux：启动时注册协议，应用移动后需要重新启动来更新路径。

建议分别测试应用退出、已打开、托盘隐藏三种状态，确认点击链接均显示一次导入窗口。再测试取消、停用导入、启用导入、重复点击、非法协议、缺少 Key，以及已有配置不变。

浏览器是否记住“允许打开应用”取决于浏览器设置；网页也无法可靠判断 XXSwitch 是否已安装，应在按钮附近提示先安装应用。
