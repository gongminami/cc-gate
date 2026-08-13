# Harness Handoff

_Last updated: 2026-08-13T18:56:00+07:00_

## Goal

构建 CC-Gate — Tauri 2 + Vue 3 桌面应用，统一管理 10 AI Agent 的模型/路由/Key/Shell。开源在 https://github.com/gongminami/cc-gate（已迁至 gongminami org）。当前 0.1.20 已发布：Gemini 直连支持 + 双端构建 + 旧版本 release 清理。

## Context you must load

- `CLAUDE.md` / `AGENTS.md`（项目根，如存在）
- `src-tauri/src/types.rs` — builtin_models()（内置模型表，含 gemini-3-flash-preview / gemini-2.5-pro）、all_api_key_names()（key 槽位表）
- `src-tauri/src/config_writer.rs` — PROVIDER_META（直连 provider 表）、write_providers（providers.json 生成）、short()（别名映射）
- `src-tauri/src/model_catalog.rs` — CATALOG_URL（远端模型目录，已指向 gongminami/cc-gate）
- `scripts/release.sh` — 发版脚本（gh auth token + gongminami/cc-gate）

## State snapshot

- branch: main @ `6ec47d5`（工作区干净，已全部 push）
- 0.1.20 已发布：https://github.com/gongminami/cc-gate/releases/tag/v0.1.20
- 旧版本 release（v0.1.0 ~ v0.1.19）已全部删除，Release 页只留 0.1.20（双包：dmg + nsis exe）
- 仓库已从 gongminami-pixel/cc-gate 迁移至 gongminami/cc-gate（org），git remote 已更新

## What works

- **Gemini 直连**：PROVIDER_META 新增 gemini（base_url=https://generativelanguage.googleapis.com/v1beta/openai，无 /v1），builtin 2 个模型（native_responses=false 走代理），中转站预设，providerLabel；cargo test 9 passed（含 gemini_provider_meta_models_and_aliases），npm run build 通过
- **双端构建 0.1.20**：macOS dmg（SHA 795261b9...）+ Windows nsis exe（SHA 492ba7e9...，VM 构建）
- **发布链路**：gh auth login 后 OAuth token（gho_）可用；release.sh 已改 TOKEN=$(gh auth token ...)
- **GitHub 清理**：17 个旧 assets + 13 个旧 release 全部删除，仅剩 v0.1.20

## What's broken

- 无已知代码卡点
- keychain 的旧 fine-grained PAT 对 cc-gate 仓库无权限（API 404），勿再当主凭据

## Next actions

1. （可选）学生机器实测 Gemini 流程：填 GEMINI_API_KEY → 模型列表选 gemini → 验证 Claude Code/Codex/Hermes 三通道
2. （可选）cc-gate 远端 models-catalog.json 补 gemini 模型条目（需编辑仓库根目录文件 + push）
3. （可选）发布脚本/产物测试：跑一次 `bash scripts/release.sh` 验证幂等（release 已存在分支）

## Open questions

- 无

## Beware

- 仓库 URL：一律用 `gongminami/cc-gate`；旧 `gongminami-pixel` 路径 API 返回 301/404，git 端跟随重定向但 API 端要 -L 或 gh CLI
- 发版前先 push 代码再跑 release.sh（tag 自动指向新 commit）
- Windows VM 构建：SSH 会话会回收子进程（npm install 被连坐杀死），必须用 `schtasks`/Task Scheduler 启动构建，日志写 VM 本地文件轮询
- 删除旧 release 时保留了 git tag（gh release delete 默认不删 tag）；用户要求彻底删 tag 需显式 --cleanup-tag
- waypoints 已 20 个（>20 提醒归档）
