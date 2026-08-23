# Harness Handoff

_Last updated: 2026-08-23T13:15:00-03:00_

## Goal

构建 CC-Gate — Tauri 2 + Vue 3 桌面应用，统一管理 10+ AI Agent 的模型/路由/Key/Shell。开源在 https://github.com/gongminami/cc-gate。当前 0.1.21 开发完成：别名体系 v2（B 方案 token 定源+切模、PI 工具接入双层 provider）+ A 方案（haiku 降级）经用户拍板撤销回归压平。待双端构建。

## Context you must load

- `CLAUDE.md` / `AGENTS.md`（项目根）
- `src-tauri/src/config_writer.rs` — build_alias_routes（别名路由表）/ merge_pi_models + write_pi_models（PI models.json 双层 provider）/ gen_aliases_impl（shell 别名行，含自定义段）
- `src-tauri/src/types.rs` — CustomAlias{name,tool,model,source} / AgentId::Pi
- `claude-proxy.js` / `chat-proxy.js` — 「方案B」注释块：aliasFor token 反查 + 尊重请求模型名；handleModels 按 token 过滤
- `src/components/PageAliases.vue` — 别名页 UI（modal 表单/列表/复制短名字）

## State snapshot

- branch: main @ 提交前工作区（本轮改动未提交 → 本次 sync 后立即提交+push）
- 版本 0.1.21（Cargo.toml 单一来源 + package.json 已 bump）
- 改动文件：config_writer/types/commands/lib/paths/backup + 两代理 js + 前端 6 文件 + PageAliases.vue 新增
- PI 本机已装（~/.pi/agent/models.json 将在下次 Apply 时被合并写入 ccgate providers）

## What works

- **B**：aliases.json 条目带 `models` 集（direct=该厂商模型集 / relay=全部启用模型）；代理 token 命中后请求模型名在集内则照传，否则回落 alias.model；/v1/models 按 token 过滤 → 别名窗口 /model 真切换
- **别名页**：添加/修改/删除即时生效（rc 重写 + 路由表热加载不重启代理）；复制按钮只复制短名字
- **PI 接入**：首页矩阵新增 PI 行；write_pi_models 合并写 ~/.pi/agent/models.json——基础层 ccgate(:8690 openai-completions) + 进阶层每别名一个 ccgate-<名>(:8689 anthropic-messages + x-api-key token 头)；保留用户自定义 provider；坏文件拒写
- **原生保护**：裸 claude 纯官方直连（单测锁死）；CC-Gate 注入全在独立别名/区块内
- cargo test 18 passed / node --check ✓ / npm run build ✓

## What's broken

- 无已知代码卡点
- keychain 旧 fine-grained PAT 对仓库无权限（用 gh auth token）

## Next actions

1. （本次同步后立即做）git 提交 feat + docs(harness) 并 push origin/main
2. 双端构建 0.1.21（macOS 前台 tauri build；Windows VM schtasks 流程见 win-vm-build skill + handoff Beware）
3. 构建产物 SHA256 留档；发布与否待用户指令（release.sh 是上线步骤，本次双端构建不含）

## Open questions

- 无

## Beware

- **A 方案撤销决策（2026-08-23 用户拍板）**：分类器(haiku档)成本占比 <5%，省钱收益几毛钱级不敌风险 → 四槽位统一钉用户选的主模型，"别名=完整一套"。haiku_model 字段/UI/路由字段已全部清除，勿再引入类似半成品开关
- mimo2codex 是外部编译二进制（proxy_manager.rs bin_dir），本仓不可改；Codex 别名走命令行直连注入绕开它
- PI 的 models.json 每次打开 /model 都热重载——Apply 后无需重启 PI 会话外的动作
- Windows VM 构建：SSH 回收子进程 → 必须 schtasks 启动 + 日志轮询；构建目录必须全新（残留伪造 Cargo.toml 会污染 include_str! 路径）；macOS tauri build 必须前台跑
- 仓库 URL 一律 gongminami/cc-gate（旧 gongminami-pixel API 404）
