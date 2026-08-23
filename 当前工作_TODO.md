# CC-Gate 当前工作 TODO

> 本轮目标：别名体系升级 —— B(token 尊重请求模型) + A(haiku 槽位降级省钱) + PI 工具接入（基础层+进阶层）
> 开始时间：2026-08-23 · 版本目标 0.1.21

## 本轮新增进度（2026-08-23 下午）

- [x] **B** 路由表条目加 `models` 集（direct=该厂商模型 / relay=全部启用模型）+ `haikuModel` 字段
- [x] **B** claude-proxy.js/chat-proxy.js：token 命中后尊重请求模型名（不在集内回落 alias.model）；/v1/models 按 token 过滤 → 别名窗口 /model 真切换
- [x] **A**（已撤销，用户拍板回归压平）：分类器省钱收益占比 <5%（几毛钱级）不敌风险与复杂度 → HAIKU 槽位统一钉主模型；haiku_model 字段/UI/路由字段/JS 分支全部清除
- [x] **PI 接入** AgentId::Pi 注册（backup.rs 两处 match 补齐）；write_pi_models 合并写 ~/.pi/agent/models.json：
  - 基础层 `ccgate` provider = 首页 pi 勾选模型，走 chat-proxy :8690 openai-completions
  - 进阶层每条别名一个 `ccgate-<名>` provider = anthropic-messages 走 claude-proxy :8689 + x-api-key token 头 → PI 窗口钉源且可切模
  - merge_pi_models 纯函数：保留用户自定义 provider；坏文件拒绝覆写
- [x] 验证：cargo test **18 passed** + node --check ✓ + npm run build ✓
- [ ] 双端构建（待用户指令）

## 上轮遗留（别名页 v1 已完成）

- 左侧菜单「别名」+ PageAliases.vue 表单/列表/modal ✓
- add/update/delete_alias 三命令 + aliases.json 路由表 + rc 自动写入 ✓

## 关键代码坐标（本轮）

- 路由 models 集: config_writer.rs build_one_route（source_models）
- haiku 降级: config_writer.rs cheapest_claude_model + haiku_slot_model + gen_aliases_impl claude 段 hk 变量
- token 尊重模型: claude-proxy.js 「方案B」注释块 / chat-proxy.js 同
- PI: types.rs AgentId::Pi / paths.rs pi_models_json / config_writer.rs merge_pi_models + write_pi_models / backup.rs Pi 分支
- PI 文档锚点: github.com/earendil-works/pi docs/models.md（models.json 热重载、anthropic-messages、headers 支持）

## 未提交状态

- 全部改动待本轮完成后本地提交（开源项目，提交后需 push origin/main）
