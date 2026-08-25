# Harness Handoff

_Last updated: 2026-08-25T01:00:00-03:00_

## Goal

构建 CC-Gate — Tauri 2 + Vue 3 桌面应用，统一管理 10+ AI Agent 的模型/路由/Key/Shell。开源在 https://github.com/gongminami/cc-gate。当前 0.2.2 双端构建完成、已装本机：Claude Code 后台任务跟随窗口主模型（tier-follow 两层防御）。待提交 + push + Release。

## Context you must load

- `CLAUDE.md` / `AGENTS.md`（项目根）
- `claude-proxy.js` — tier-follow 核心：windowMainModel Map（约 702-717 行记录主模型）+ retarget（约 845-870 行，tier 形状请求重定向到主模型）
- `src-tauri/src/config_writer.rs` — write_shell_aliases 内 claude-cc-gate 段的哨兵变量 claude-haiku-follows-main
- `scripts/test-claude-proxy.cjs` — 33 项测试（含 4 个 tier-follow 用例），改 claude-proxy.js 必跑
- `scripts/release.sh` — 发版脚本（版本四处硬编码已更新为 0.2.2）

## State snapshot

- branch: main @ origin/main 同步（4288798 docs README），本轮未提交改动 = 0.2.2 全部内容
- 版本 0.2.2（Cargo.toml 单一来源 + package.json 元数据）
- 双包就绪：dmg 在 src-tauri/target/release/bundle/dmg/CC-Gate_0.2.2_x64.dmg（SHA256 c170d489…72ed8）；exe 在 /tmp/CC-Gate_0.2.2_x64-setup.exe（SHA256 503e77c6…f232）
- 已装本机 /Applications/CC-Gate.app 0.2.2（卷内验证 + 前端 hash 嵌入自检通过；旧版备份 .bak-20260825-004734 也是 0.2.2=断线前中间构建）
- v0.2.1 Release 已发（2026-08-24，GitHub 上 Latest）；v0.2.2 未发

## What works

- **tier-follow（0.2.2 核心）**：后台任务（权限分类器/话题检测/标题生成）按官方 tier 名发的请求自动重定向到当前窗口主模型；中转站真名 claude-opus-5 有路由永不误判；sk- 开头真实 key 的官方窗口不劫持；冷启动非别名 token 回落 TOKEN_MAP/内置 deepseek-v4-pro
- **哨兵变量**：统一命令四 tier env 钉 claude-haiku-follows-main，后台任务抢跑也被代理重定向，杜绝冷启动 401
- node 测试 33/33 ✓；npm run build ✓（vue-tsc 零错误）；双端产物验证通过
- 0.2.x 统一网关架构全量功能（见 git log 752b2c3）

## What's broken

- 无已知代码卡点
- keychain 旧 fine-grained PAT 对仓库无权限（用 gh auth token）

## Next actions

1. git 提交：feat（源码 6 文件）+ docs(harness)（.harness/）+ release.sh → push origin main
2. bash scripts/release.sh 发 v0.2.2（先 push 再发版——tag 自动指向最新 commit 的铁律）
3. gh release view v0.2.2 --repo gongminami/cc-gate 验证资产与 body

## Open questions

- 无

## Beware

- **仓库 URL 一律 gongminami/cc-gate**（旧 gongminami-pixel API 404）
- **每次出包 bump 小版本**（Cargo.toml 单一来源）；同号异容禁止
- **tauri bundle_dmg.sh 失败不再算偶发**（连续两撞）：手动绝对路径跑 bundle_dmg.sh 一次过；失败会在 /Volumes 积 dmg.XXXX 残留挂载点，重跑前必须 detach 干净
- Windows VM 构建：代理重启会杀 Mac 侧 ssh 后台进程但 VM 侧构建存活——续跑先查 VM 产物目录再决定是否重建；VM 构建必须 schtasks 或接受 ssh 断线风险（本次 ssh 直跑侥幸存活）
- deploy_proxy_scripts 覆盖磁盘调试 js：改 claude-proxy.js 必须重建才进 app
- reqwest 未开 json feature：解析用 .text()+serde_json
- mimo2codex 是外部编译二进制，本仓不可改
