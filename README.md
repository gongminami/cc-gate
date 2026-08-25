# CC-Gate

**一个桌面应用，统一管理你所有 AI 编程工具的模型配置。**

v0.2 起进入「统一网关架构」：每个工具一条命令，打开就是全量模型列表——官方模型、直连厂商、中转站模型全部混排，切模型在工具内完成，不再需要为每个组合预制命令。

---

## 🚀 快速开始（三步）

```
① 添加 API Key 或中转站        → 「中转与API_Key」页
② 点「发现模型」并挑选          → 中转站几百个模型一键导入，勾掉不要的
③ 复制统一命令，终端里敲        → claude-cc-gate
```

完成。Claude Code 里输入 `/model`——官方 Claude、DeepSeek、GLM、Qwen、Kimi、中转站模型全部在列，随时切换。

## ⌨️ 五条统一命令

| 工具 | 命令 | 说明 |
|------|------|------|
| Claude Code | `claude-cc-gate` | /model 切换全部模型 |
| Codex CLI | `codex-cc-gate` | 完整模型目录，Codex 内切换 |
| Aider | `aider-cc-gate` | 以第一个启用模型启动 |
| Hermes | `hermes-cc-gate` | `-m` 参数随时切模型 |
| PI | `pi-cc-gate` | 全部启用模型自动写入其配置 |

裸命令（`claude` / `codex` / `aider`）保持官方原生直连，完全不受 CC-Gate 影响。

## 🆚 与 CC Switch 的区别

**CC Switch 是"先选店再点菜"，CC-Gate 是"所有菜在一张菜单上"。**

CC Switch 有"供应商"中间层：用什么模型取决于当前激活了哪个供应商，切换是全局的、要退出重开。CC-Gate 没有供应商概念——代理按模型名自动路由到对应厂商，所有工具共享一张全量菜单，多窗口可以同时跑不同厂商的模型。

## 🌐 中转站模型发现

支持任何 OpenAI 兼容中转站（OpenRouter、商汤日日新、自建网关……）：

1. **添加**：填 URL 和 Key（内置 OpenRouter 预设）
2. **发现**：一键拉取该站全部模型（OpenRouter 实测 400+），默认全选导入
3. **挑选**：弹窗内搜索、按厂商分组批量勾选——只有勾选的才会出现在工具列表里
4. **启停**：不续费的中转站一键隐藏，重新启用即恢复

发现的模型以 `OpenRouter/deepseek/deepseek-v3.2` 这样的带站名 ID 出现在所有工具里，一眼区分来源。

## 📦 三类模型来源

| 来源 | 进入方式 | 管理位置 |
|------|---------|---------|
| 官方目录 | 远端自动下发，厂商出新模型免重装 | 「模型管理」页检查更新 |
| 中转站发现 | 一键拉取 + 挑选 | 「中转与API_Key」页 |
| 自定义模型 | 手动添加小众平台 | 「模型管理」页 |

每个目录模型还可单独设置**线路**（官方直连或走某个中转站），对所有工具生效。

## 🔧 更多功能

- **桌面端接入**：Codex Desktop / Claude Desktop 图形化配置写入与恢复备份
- **高级别名**（可选）：把某个窗口锁死在一个来源上；同一模型多来源并行对比
- **API Key 管理**：22 个提供商统一管理，Key 只写本地 `.env`
- **应用更新检查**：启动时静默对比 GitHub Releases，有新版红点提醒
- **系统代理兼容**：GitHub 访问自动走 macOS 系统代理
- **热重载**：发现/挑选/改线路即时生效，无需重启代理或终端
- **工具检测 / 启动项管理**：依赖检测、开机自启

## ❓ 常见问题

**提示 Connection error？**
`*-cc-gate` 命令的流量经过 CC-Gate 管理的本地代理（127.0.0.1:8688/8689/8690）。CC-Gate 必须保持运行（最小化即可），完全退出软件 = 断开路由。

**中转站的模型有的能用有的报错？**
中转站目录里的模型 ≠ 都能用于 coding agent。常见限制：Anthropic/OpenAI 系大厂模型被平台 ToS 拦截、部分模型不支持工具调用消息角色（agent 必需）、部分模型需在平台上单独开通。用「挑选」只保留实测可用的即可。

**模型 ID 前面的 `OpenRouter/` 是什么？**
中转站模型的显示 ID 带**站名前缀**，用于区分来源。转发时会自动剥掉前缀，不影响实际调用。

**官方 Claude 模型能直接用吗？**
模型列表里会展示官方 Claude 全系。使用它们需要你已登录官方账号或配置官方 API Key（与第三方中转无关）。

**Windows 上能用吗？**
可以，v0.2.1 提供双平台安装包，功能一致。

## 支持的工具

| 工具 | 类型 | 本地代理 | 接入方式 |
|------|------|---------|---------|
| Claude Code | CLI | :8689 网关发现 | `claude-cc-gate` |
| Codex CLI | CLI | :8688 目录 | `codex-cc-gate` |
| Aider | CLI | :8690 | `aider-cc-gate` |
| Hermes | CLI | :8690 | `hermes-cc-gate` |
| PI | CLI | :8690 配置写入 | `pi-cc-gate` |
| Codex Desktop / Claude Desktop | 桌面端 | 同端口 | 图形化配置写入 |

## 安装

从 [Releases](../../releases) 下载最新版 `.dmg`（Mac）或 `.exe`（Windows），拖入应用程序文件夹即可。软件内置更新检查，有新版会自动提醒。

### 从源码构建

```bash
git clone https://github.com/gongminami/cc-gate.git
cd cc-gate
npm install
bash scripts/build-mac.sh   # macOS（含前端嵌入缓存修复）
```

## 安全

- API Key 存储在本地 `~/.mimo2codex/.env`，不落明文配置
- 所有代理仅监听 `127.0.0.1`，不接受外部连接
- 无遥测、无数据上传

## 许可证

MIT

## 致谢

设计深受 [CC Switch](https://github.com/cexll/myclaude) 启发——并将"全局供应商切换"推进到"统一网关 + 全量模型菜单"。

---

**CC-Gate — 一个 GUI 管所有 AI 工具，一条命令一个全量模型菜单。**
