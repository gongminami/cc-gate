# Harness Decisions Ledger

_Append-only. Each entry captures the "why" behind a choice._

---

## 2026-07-25T22:45:00+08:00 — 启用 harness-framework
**Why**: 跨对话/跨模型/跨上下文压缩的零漂移长记忆
**What**: 建立 .harness/ 通过 harness-framework skill 管理会话状态
**Alternatives**: 纯 CLAUDE.md + auto-memory — 缺结构化会话级状态
**Evidence**: -
**Supersedes**: -
**Impact**: 后续会话通过"读取记忆"等触发词 boot

## 2026-07-25T23:00:00+08:00 — 工具检测挪到左侧独立菜单项
**Why**: 用户认为放在首页底部不合适，应该像"启动项"一样独立
**What**: 新建 PageTools.vue，左侧菜单加"🔧 工具检测"项（启动项上面）
**Alternatives**: 保留在首页底部卡片、做成弹窗
**Evidence**: src/components/PageTools.vue, src/components/Sidebar.vue:12
**Supersedes**: -
**Impact**: 首页不再堵塞等待工具检测；用户点左侧菜单主动查看

## 2026-07-25T23:00:00+08:00 — 工具检测加 OnceLock 缓存
**Why**: 每次进首页都跑 6 个外部命令卡 2 秒，工具不会频繁装卸
**What**: Rust 侧 OnceLock<Mutex<Vec<ToolStatus>>>，首次跑后缓存
**Alternatives**: 前端 localStorage 缓存 -- 不准确（前端不知道真实状态）
**Evidence**: src-tauri/src/tool_check.rs:19-35
**Supersedes**: -
**Impact**: 后续调用无 IO 开销；提供 refresh() 手动清缓存

## 2026-07-25T23:00:00+08:00 — 首页应用按钮 dirty-aware
**Why**: 用户要求"只有改了设置才能点，没改就灰着"
**What**: computed dirty 对比 workingModels/modelRouting 与原始 config
**Alternatives**: 手动 watch 各个字段 -- 太碎
**Evidence**: src/components/PageHome.vue:22-38
**Supersedes**: -
**Impact**: 用户视觉上知道是否需要点应用

## 2026-07-25T23:00:00+08:00 — Codex CLI / Claude CLI / Aider writes_providers=true
**Why**: CLI Agent 的模型必须进 providers.json，否则代理不认识、切换模型时报 stream disconnected
**What**: 三个 CLI Agent 的 writes_providers 改为 true
**Alternatives**: 只靠桌面端的 writes_providers -- CLI 模型不完整
**Evidence**: src-tauri/src/types.rs:36,38,43
**Supersedes**: -
**Impact**: /model 命令能看到完整模型列表，切换不断流

## 2026-07-26T01:30:00+08:00 — 远程模型目录自动更新（解决厂商出新模型需重编 CC-Gate 的问题）
**Why**: 用户问"厂商出新模型咱们的程序还起作用么"——builtin_models() 硬编码 9 个模型，新模型必须改代码 + 重编 + 出包。用户要求不依赖 CC-Gate 发版就能跟上厂商更新
**What**: 
  - models-catalog.json 放仓库根目录 → GitHub raw URL 可访问
  - 新增 model_catalog.rs：fetch_remote_catalog (reqwest HTTPS) + 本地缓存 (~/.mimo2codex/models-cache.json) + merge_remote_models
  - merge 策略：远程参数覆盖本地（context_window/pricing 等），但保留用户 enabled 状态；远程新模型默认 enabled=false
  - 启动时后台静默拉取（不阻塞 UI）
  - 首页"检查模型更新"按钮供用户主动刷新
  - 离线兜底链：缓存 → builtin_models()
**Alternatives**: 
  - 不从远程拉，纯依赖定���发版更新 builtin_models() ——太重
  - 从代理 /v1/models 动态发现——代理端 /v1/models 返回不完整，且 Rust 侧没有消费代码
  - JSON 放独立仓库——当前放主仓库根目录，简单够用
**Evidence**: models-catalog.json, src-tauri/src/model_catalog.rs, src-tauri/src/config_store.rs:13-31
**Supersedes**: builtin_models() 作为唯一模型源（现降级为兜底）
**Impact**: 厂商出新模型只需改 models-catalog.json + git push；所有 CC-Gate 实例自动获取。builtin_models() 保留不动作终极兜底

## 2026-07-26T01:30:00+08:00 — 侧边栏隐藏用量统计和模型参数
**Why**: 用户说模型参数不准确（context_window 等没校准），用量统计逻辑也未启用
**What**: 注释掉 Sidebar.vue 中 usage 和 models 两个菜单项；两个 proxy .js 中 recordUsage() 调用注释掉。代码保留不删，以便将来校准后恢复
**Alternatives**: 删除代码——将来恢复需从 git history 找回
**Evidence**: src/components/Sidebar.vue:9-10, claude-proxy.js:364,390, chat-proxy.js:291
**Supersedes**: -
**Impact**: 用户看不到这两个菜单项；用量 jsonl 不再写入

## 2026-07-26T11:00:00+08:00 — 工具检测改为逐条 IPC 调用实现渐进式加载
**Why**: emit event 方案在 Tauri 中失效——同一个 command 执行期间 event 被缓冲，command 返回后才批量送达前端（thread::spawn 异步也不行）
**What**: 废弃 streaming command，改为 6 个独立 IPC——前端 async for loop 调用 checkOneTool()，每次 await 后立即更新 UI，最后 saveToolCache 回写缓存
**Evidence**: src-tauri/src/tool_check.rs:36-44, src-tauri/src/lib.rs:189-193, src/components/PageTools.vue:48-63
**Supersedes**: 旧版 check_tools() 一次性返回（保留备用）
**Impact**: 6 次 IPC 替代 1 次，换取实时流式体验

## 2026-07-26T12:00:00+08:00 — 模型参数更新：Opus 5 + GPT-5.6 + GLM 1M 上下文
**Why**: 用户指出 Opus 最新 5.0（1M）、GPT 最新 5.6、GLM 支持 1M
**What**: slug claude-opus-4-5→claude-opus-5, gpt-5.1-codex→gpt-5.6; GLM context 128K→1M; 同步更新 models-catalog.json + builtin_models() + short() + README
**Evidence**: types.rs:147-148, models-catalog.json:53-58, config_writer.rs:594
**Supersedes**: 旧模型版本参数
**Impact**: alias 名不变（short() 仍返回 "opus"/"gpt"），但 slug 和参数已是新版

## 2026-07-26T14:20:00+08:00 — 3 代理 App 打开即无条件启动（autostart 开关废弃）
**Why**: 用户要求软件打开时 3 个代理始终运行监听，不需要手动开关
**What**: proxy_manager.rs start_enabled() 无条件启动 3 代理；apply_agent_config 无条件重启 3 代理
**Evidence**: proxy_manager.rs:107-115, commands/config.rs:31-51
**Supersedes**: 旧 autostart 开关控制启动逻辑

## 2026-07-26T14:20:00+08:00 — node 路径自动发现 + mimo2codex 同 bin 目录查找
**Why**: macOS GUI app 不继承 shell PATH；nvm 多版本并存时需找对版本（有 node + mimo2codex 的）
**What**: find_node() 扫 nvm/fnm/volta/Homebrew，优先选有 node+mimo2codex 的版本；proxy_script_for() 中 mimo2codex 走 bin_dir
**Evidence**: proxy_manager.rs:17-100
**Supersedes**: 裸 "node" 命令（永远找不到）+ 裸 "mimo2codex" 脚本名（当模块找→Module not found）

## 2026-07-26T14:20:00+08:00 — providers.json 加 defaultModel 字段
**Why**: mimo2codex 的 genericLoader 要求每个 provider 必须有 defaultModel，否则启动退出码 2
**What**: config_writer.rs write_providers() 中每个 provider entry 加 "defaultModel": models[0].slug
**Evidence**: config_writer.rs:95-107
**Supersedes**: 旧 providers.json（无 defaultModel→mimo2codex 启动失败）

## 2026-07-26T14:20:00+08:00 — 首页应用按钮断连保护
**Why**: 用户点"应用"会重启 claude-proxy→断开当前 CC Chat 会话
**What**: onApply() 前检测 claude-proxy 是否将重启→弹 window.confirm 警告
**Evidence**: PageHome.vue:80-91
**Supersedes**: 无保护→点应用直接断连

## 2026-07-26T14:20:00+08:00 — 代理存活性双验证（try_wait + 端口监听）
**Why**: spawn() 成功≠进程真活着；旧 CC-Gate 崩溃后遗留进程占端口
**What**: status() 加 try_wait 清理死 Child + port_is_listening() TCP connect 兜底；start() 前 lsof kill 清端口
**Evidence**: proxy_manager.rs:104-119, 216-260
**Supersedes**: 旧 HashMap-only 检测（假活）

## 2026-07-28T17:30:00+08:00 — Windows 构建必须用全新目录，否则 Tauri 自动生成 Cargo.toml 污染
**Why**: Tauri CLI (`tauri.js build`) 在项目根自动生成 Cargo.toml（`path = "src/main.rs"`），后续编译时 `include_str!("../../...")` 从 `src/` 解析成错误路径（项目根父目录）。残留的伪造 Cargo.toml 不删则每次构建都失败
**What**: 每次 Windows 构建前 `rmdir /s /q` 清旧 build 目录，新建后 tar 解压，确保无残留文件
**Alternatives**: 改 include_str! 绝对路径、从 src-tauri 目录编译——前者不跨平台，后者 bundler 仍从项目根调 cargo
**Evidence**: cc-gate-build 目录成功；cc-x-llm 目录因残留 Cargo.toml 反复失败
**Supersedes**: -

## 2026-07-28T18:00:00+08:00 — 中转站弹窗改为 Modal 模式
**Why**: 四个输入框内联在页面上视觉混乱，添加/编辑是偶发操作不需要常驻
**What**: PageRelayKeys.vue 去掉 `.add-relay-box`，改为 Modal overlay（`v-if="showRelayModal"`），四个字段纵向排列，点遮罩关闭
**Alternatives**: 做成展开/折叠面板——仍然占空间
**Evidence**: src/components/PageRelayKeys.vue
**Supersedes**: 旧内联表单布局

## 2026-08-02T12:16:00+08:00 — CC-Gate 为开源项目，同步提交后必须 push
**Why**: 本项目开源在 https://github.com/gongminami-pixel/cc-gate，与用户其他闭源/私有项目不同。"同步加提交"后代码仅在本地，其他人（包括用户的其他设备）看不到——必须 git push origin main
**What**: 全局 CLAUDE.md 约定"同步加提交=只本地不 push"对 CC-Gate 不适用；每次本地提交后需额外 push 到 GitHub
**Alternatives**: 改全局 CLAUDE.md ——但那是用户所有项目的通用规则，不能为单项目改动
**Supersedes**: 全局 CLAUDE.md "同步加提交=永不 push"（仅限本项目覆盖）
**Impact**: 每次"同步加提交"后，若用户未明确说"只本地"，需主动 push 到 origin/main

## 2026-08-13T13:25:00+08:00 — deepseek-v4-pro 原生 Responses，mimo2codex 保留
**Why**: deepseek-v4-pro 实测已原生支持 Responses API（curl /v1/responses + codex exec 直连通过），不再需要 mimo2codex 做 Responses→Chat 翻译
**What**: types.rs 里 deepseek-v4-pro 设 native_responses=true；codex-ds 别名直连 api.deepseek.com
**Evidence**: curl /v1/responses 返回 object:response；codex exec 直连返回 OK
**Supersedes**: 旧「deepseek-v4-pro 走代理模式」（cc-gate skill DeepSeek 连接模式表）
**Impact**: mimo2codex(8688) 不删除——GLM/Qwen/MiMo 无 Responses 接口（GLM /responses 实测 404），经 Codex 时仍需 8688 翻译

## 2026-08-13T13:25:00+08:00 — 版本号三处同步 + Sidebar 动态读取
**Why**: Sidebar 左下角硬编码 v0.1.0（从未跟版本走）；Cargo.toml version 停在 0.1.2 与 tauri.conf.json 脱节
**What**: Sidebar 改 getAppVersion()（读 package_info.version = tauri.conf.json version）；Cargo.toml version 同步 0.1.19
**Evidence**: src/components/Sidebar.vue:33 原硬编码 v0.1.0；App.vue 里 'settings' 页 = PageAbout
**Supersedes**: 硬编码版本号
**Impact**: 后续发版只需改 tauri.conf.json + Cargo.toml 两处，UI 自动显示

## 2026-08-23T13:15:56-0300 — A 方案（haiku 分类器降级）实现后撤销，回归四槽位统一压平
**Why**: 分类器(haiku档)调用都是几百 token 小请求，成本占比 <5%；DeepSeek flash/pro 单价差下每月仅省几毛钱，不敌弱模型伤后台任务质量、统计混名、跨源兼容边界三个真实代价。用户拍板"保险优先"
**What**: haiku_slot_model/cheapest_claude_model 删除；CustomAlias.haiku_model、路由表 haikuModel 字段、JS 分支、前端下拉全链清除；别名语义定版 = "一条别名=完整一套（工具×模型×源，四槽位统一）"
**Supersedes**: 2026-08-23 早间"haiku 槽位降级省钱"方案
**Evidence**: config_writer.rs gen_aliases_impl claude 段 / custom_claude_line

## 2026-08-23T13:15:56-0300 — 别名路由 B 方案 + PI 双层 provider 架构
**Why**: 全局 providers.json 按模型名唯一路由 → 同模型多窗口必同源；PI 无 shell env 注入机制但有原生自定义 provider 体系（models.json 热重载、支持 anthropic-messages 协议）
**What**: ①aliases.json 条目带 models 集（direct=该厂商集/relay=全部启用），代理 token 命中后尊重集内请求模型名否则回落绑定模型——/model 真切换且列表按 token 过滤；②pi: 基础层 ccgate(:8690 openai-completions 跟首页矩阵) + 进阶层每别名一个 ccgate-<名>(:8689 anthropic-messages + x-api-key token 头)；merge 保留用户 provider、坏文件拒写
**Alternatives**: 每别名独立代理端口（端口爆炸弃）；纯 env 直连中转站（丢统计+协议翻译不通弃）
**Evidence**: claude-proxy.js「方案B」块 / config_writer.rs build_alias_routes + merge_pi_models / github earendil-works/pi docs/models.md
