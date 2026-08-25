# Claude Code 模型列表统一方案（网关模型发现）

> 状态：方案设计（待评审，2026-08-23 讨论确认，待有空实施）
> 日期：2026-08-23
> 范围：CC-Gate 的 claude-proxy(:8689) + config_writer.rs
> 目标：打开 Claude Code，/model 选择器能看到「官方 Claude 模型 + 直连厂商全部模型（DeepSeek/智谱/千问/MiMo/LongCat）+ 各中转站模型」，且切换后真实可用

---

## 〇、背景与缘起（为什么做这件事）

### 0.1 上游问题：opencode 模型列表混乱（2026-08-22 排查）

用户 opencode 里能切 59 个模型，来源三类：
1. CC-Gate 自定义 provider 白名单 7 个（opencode.jsonc 配置决定）
2. 智谱系 40 个——shell 里 `ZHIPU_API_KEY` 环境变量被 opencode 映射到 4 个 provider 分组（zhipuai/zhipuai-coding-plan/zai/zai-coding-plan），每组把 models.dev 目录全量列出
3. 其他凭据 12 个（DeepSeek key 5 个 + opencode 官方免费 7 个）

实测结论（opencode 1.18.21）：内置 provider 的模型列表 = models.dev 全量 + 配置只加不减；自定义 provider 才听配置白名单。所以精简只能删凭据（用户选择不删）。

### 0.2 统一网关愿景（2026-08-23 讨论）

用户提出根治方向：CC-Gate 添加中转站后，每个工具（CLI）打开切换模型时，都能看到「官方直连全部模型 + 各中转站全部模型」，统一管理、开箱即用。

结论：
- **Codex CLI / ChatGPT 桌面端 / opencode / Hermes**：走 /v1/models 聚合端点或配置白名单，完全可行
- **Claude Code 最特殊**：/model 选择器 UI 列表官方不给注入口，只能靠它的「网关模型发现」机制（见 §2.1）
- 中转站模型枚举是唯一硬坎：有 /v1/models 的中转站可自动拉取合并，没有的只能手工录入

### 0.3 本方案定位

本文档 = 统一网关愿景中 **Claude Code 专项处理**（讨论时确认"对 Claude Code 做特殊处理"）。其他工具的处理方案另行设计（Codex 走 8688 模型目录、opencode 走配置白名单、桌面端走 catalog，均已有基础）。

### 0.4 UI 影响：首页 / Shell 集成页的未来形态（2026-08-23 讨论）

用户问：方案完善后，首页和 Shell 集成页是否基本没用？结论：**大幅瘦身，但不会消失——职责转移，不是淘汰**。

**Shell 集成页：接近"没用了"，但保留机制**
- 现状：核心是展示「每个模型一个 alias」（codex-ds / codex-glm / claude-ds / claude-glm / aider-mimo…），模型越多 alias 越多
- 方案后：工具侧自己发现模型（Claude Code 靠 /model 切换、Codex 靠模型目录、opencode 靠白名单），不再需要"每模型一个 alias"——每工具一个 alias（指向代理 + 开 discovery）就够，切模型在工具里切。页面缩水为"alias 已写入 ~/.zshrc，共 3 条"的状态卡
- alias 机制不会消失：alias 承载的不只是选模型，还有 ANTHROPIC_BASE_URL/TOKEN 等"指向代理"的配置 + "无后缀 alias = 官方原生"的设计

**首页：不会没用，职责转移**
- 弱化：给每个 agent 勾选模型列表（agent_models 勾选）——模型发现后 agent 侧全量可见，"勾选白名单"语义淡化
- 保留（真身，三块）：
  1. 代理进程管理——8688/8689/8690 启停、状态灯、重试，永远需要
  2. 模型路由配置（direct / relay:中转站）——自动发现只解决"列表看得到"，不解决"切了发到哪"。路由表是统一网关的核心，首页是它的编辑界面
  3. 中转站管理 + 应用/恢复/备份——加中转站、点应用生成配置、一键恢复，不会消失

**一句话总结**：首页从「配置分发中心」（给每个工具分配模型）→「路由管理 + 代理运维中心」（管模型发到哪、管代理活没活）；Shell 页从「alias 清单」→「alias 状态卡」。真正消失的是"每个模型生成一条 alias"这个中间产物——被模型发现机制取代，不是被页面取代。

---

## 一、用户诉求

原生 Claude Code 只有官方那几个模型。使用 CC-Gate 方案后，希望打开 Claude Code 的模型切换器时能看到：

1. Claude 官方模型（Opus/Sonnet/Haiku 等）
2. 官方直连厂商的全部模型：DeepSeek 全系、智谱 GLM 全系、千问、小米 MiMo、美团 LongCat
3. 每个中转站对接的全部模型（中转站分组展示）

切换任意模型都能真实工作（路由到正确的上游）。

## 二、现状调研（2026-08-23 实测）

### 2.1 Claude Code 官方机制：网关模型发现（已存在，官方支持）

Claude Code 从 2.x 起支持网关模型发现：
- 环境变量 `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`（opt-in）
- 当 `ANTHROPIC_BASE_URL` 指向 Anthropic 兼容网关时，/model 选择器**列出网关 `/v1/models` 端点返回的模型**
- changelog 原文：`The /model picker now lists models from your gateway's /v1/models endpoint when ANTHROPIC_BASE_URL points at an Anthropic-compatible gateway`

CC-Gate 生成的 Claude alias **已经注入该环境变量**（config_writer.rs:878 等，`CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`），机制链路是通的。

### 2.2 claude-proxy.js 已有 /v1/models 端点

`claude-proxy.js:639-660` `handleModels()` 已实现网关发现响应：

```js
const models = providers.map(p => ({
  id: 'claude-' + p.defaultModel,   // claude- 前缀是 Claude Code 要求
  type: 'model',
  display_name: p.displayName,
  created_at: '2025-01-01T00:00:00Z',
  context_window: p.contextWindow || 200000,
  max_output_tokens: p.maxOutputTokens || 16384,
}));
res.end(JSON.stringify({ data: models }));
```

实测当前返回（无 token）：

```
claude-deepseek-v4-pro      DeepSeek V4 Pro
claude-deepseek-v4-flash    DeepSeek V4 Flash
claude-glm-5.2              GLM-5.2
claude-gpt-5.6              GPT-5.6
claude-qwen3.8-max-preview  Qwen3.8 Max Preview
claude-mimo-v2.5-pro        MiMo V2.5 Pro
```

### 2.3 模型→上游路由链（已通）

请求 `/v1/messages` 时（claude-proxy.js:710-748）：
1. 剥 `claude-` 前缀（`modelId.slice(7)`，先查 PROVIDERS 再剥，防 `claude-claude-*` 误剥）
2. `PROVIDERS[realModelId]` 命中 → 按 providers.json 路由到对应上游（直连厂商或中转站）
3. 未命中且是 `claude-(opus|sonnet|haiku|fable)-` → Anthropic 官方透传（754 行）

### 2.4 providers.json 结构

`~/.mimo2codex/providers.json` 每个条目含 `models[]` 数组（config_writer.rs:204-209），loadProviders 展开为 `PROVIDERS[model.id]` 平铺表。**多模型天然支持**——一个 provider 条目下挂 N 个模型，handleModels 会全部返回（当前每个条目只有 1 个模型是用户配置现状，不是代码限制）。

## 三、差距分析（要做的事）

| # | 差距 | 现状 | 影响 |
|---|------|------|------|
| G1 | 官方 Claude 模型不在发现列表 | handleModels 只遍历 PROVIDERS（providers.json），官方模型无条目 | 用户开了 discovery 后**看不到官方模型**，反而比原生列表少 |
| G2 | 官方模型透传 model 名错误 | 754-760 行透传时 `reqBody.model = realModelId`（已剥 `claude-` 前缀），Anthropic API 要求完整 `claude-opus-4-5` | 切到官方模型会 400 model not found |
| G3 | display_name 无分组信息 | 用 provider 的 displayName，平铺显示 | 模型一多分不清厂商/中转站归属 |
| G4 | alias 窗口过滤只认 defaultModel | 647-648 行 `allowed.has(p.defaultModel)`，多模型 entry 下过滤可能误伤 | 中转站别名窗口模型显示不全 |
| G5 | 无官方模型清单来源 | 硬编码 or 动态读取？ | 官方模型列表需要维护 |

## 四、方案设计

### 4.1 官方模型清单（解决 G1/G5）

**推荐：在 claude-proxy.js 内维护一份官方模型常量表**（轻量、无网络依赖、随 proxy 部署）：

```js
// ── Anthropic official models (gateway discovery supplement) ──
const OFFICIAL_MODELS = [
  { id: 'claude-opus-4-5',      display_name: 'Claude Opus 4.5',     context_window: 200000,  max_output_tokens: 32000 },
  { id: 'claude-sonnet-4-5',    display_name: 'Claude Sonnet 4.5',   context_window: 200000,  max_output_tokens: 64000 },
  { id: 'claude-haiku-4-5',     display_name: 'Claude Haiku 4.5',    context_window: 200000,  max_output_tokens: 32000 },
  // 按需增补（如 claude-fable-* 系）
];
```

要点：
- id 保持 `claude-` 前缀（与 handleModels 输出格式一致，Claude Code 直接显示）
- 版本更新时在此表增删条目即可，**单一来源**，不散落
- 官方模型走 754 行已有的 Anthropic 原生透传（需 G2 修复），不需要 providers.json 条目

### 4.2 修复官方模型透传 model 名（解决 G2）

`claude-proxy.js:754-760`，透传时用**原始 modelId**（带 `claude-` 前缀），不用已剥前缀的 realModelId：

```js
// 修复前
reqBody.model = realModelId;   // 'opus-4-5' → Anthropic API 400
// 修复后
reqBody.model = modelId;       // 'claude-opus-4-5' → 官方 API 正确
```

### 4.3 handleModels 合并官方模型 + 分组排序（解决 G1/G3）

改造 `handleModels()`：

```js
function handleModels(res, token) {
  const alias = aliasFor(token);
  let providers = Object.values(PROVIDERS);
  if (alias) {
    const allowed = new Set(Array.isArray(alias.models) ? alias.models : [alias.model]);
    providers = providers.filter(p => allowed.has(p.defaultModel));
  }
  // G4 修复：多模型 entry 下按 models 数组过滤，而非只看 defaultModel
  // （loadProviders 已把每个 model 展开成独立条目，实际 P.defaultModel === model.id，
  //   此处逻辑已正确，保留即可）

  // ① 官方模型打头（仅无 alias 或 alias 显式放行官方模型时）
  const official = OFFICIAL_MODELS.map(m => ({
    id: m.id,
    type: 'model',
    display_name: m.display_name,
    created_at: '2025-01-01T00:00:00Z',
    context_window: m.context_window,
    max_output_tokens: m.max_output_tokens,
  }));

  // ② 直连/中转站模型，display_name 带分组前缀
  const routed = providers.map(p => ({
    id: 'claude-' + p.defaultModel,
    type: 'model',
    display_name: p.displayName,   // G3：上游已含 " via <relay>" 后缀（config_writer 拼的）
    created_at: '2025-01-01T00:00:00Z',
    context_window: p.contextWindow || 200000,
    max_output_tokens: p.maxOutputTokens || 16384,
  }));

  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ data: [...official, ...routed] }));
}
```

分组显示策略：
- **官方模型**排最前，天然一组
- **直连厂商**：displayName 已经是厂商名（"DeepSeek V4 Pro"），平铺自然分组
- **中转站**：config_writer.rs:185 已拼 `" via <relay名>"` 后缀（如 "GLM-5.2 via 某中转站"），Claude Code 列表里天然可见归属
- Claude Code 的 /model 选择器是**平铺列表**（无 opencode 那种分组树），分组靠 display_name 前缀/后缀表达——这是客户端原生限制，方案不追求 UI 分组树

### 4.4 alias 窗口语义（不动的部分）

alias 窗口（如 `claude-ds` 这种带 token 的）继续只显示该 source 携带的模型——避免列出切过去必然 400 的名字（handleModels 注释里的设计意图，保留）。

### 4.5 config_writer.rs 侧配合（可选增强）

当前 `write_claude_settings()` 不写 model 字段（bug #2 已修复，settings.json 的 model 必须是 tier）。**无需改动**——模型选择完全由 alias 环境变量 + discovery 列表承担。

中转站接入后只需「应用」一次，providers.json 自动带上新模型，/v1/models 立即生效（PROVIDERS 每次请求热读 mtime，见 refreshAliases/loadProviders 机制——注意 PROVIDERS 是启动快照，新增模型需重启 8689 或等 deploy_proxy_scripts 重启）。

## 五、验证清单

```bash
# 1. 发现列表含官方模型 + 全部路由模型（期望 6 + N 官方）
curl -s http://127.0.0.1:8689/v1/models | python3 -m json.tool

# 2. 官方模型透传（期望不再 400，返回 Anthropic 官方响应或认证错误而非 model not found）
curl -s http://127.0.0.1:8689/v1/messages \
  -H 'Content-Type: application/json' -H 'x-api-key: test' \
  -d '{"model":"claude-opus-4-5","max_tokens":15,"messages":[{"role":"user","content":"hi"}]}'

# 3. 直连模型路由不回归
curl -s http://127.0.0.1:8689/v1/messages \
  -H 'Content-Type: application/json' -H 'x-api-key: ds' \
  -d '{"model":"claude-deepseek-v4-pro","max_tokens":15,"messages":[{"role":"user","content":"hi"}]}'

# 4. Claude Code 实测：/model 选择器应显示官方组 + 全部路由模型
ANTHROPIC_BASE_URL=http://127.0.0.1:8689 ANTHROPIC_AUTH_TOKEN=proxy \
  ANTHROPIC_MODEL=claude-deepseek-v4-pro \
  CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 \
  claude -p "say hi"

# 5. 自动化测试（改 claude-proxy.js 后必跑）
cd 项目根 && node scripts/test-claude-proxy.cjs
```

## 六、边界与限制（如实声明）

1. **Claude Code 选择器是平铺列表**，没有 opencode 那种按 provider 分组树；分组靠 display_name 后缀（"via 中转站"）表达
2. **官方模型需要真实 Anthropic key/OAuth** 才能用——列表会显示，但未登录官方账号时调用会 401（正常现象，不是 bug）
3. **Claude 桌面端 App（聊天）不受此方案影响**：OAuth 登录后直连 api.anthropic.com，绕过 8689（已知行为，见项目记录）
4. providers.json 变更（新增中转站/模型）需要 8689 重启才生效（PROVIDERS 启动快照，热读只覆盖 .env 和 aliases.json）
5. 官方模型清单是**静态常量表**，Anthropic 发布新模型需更新 OFFICIAL_MODELS（可在 config_writer.rs 或远端 catalog 扩展为可配置，二期再说）

## 七、实施步骤（按依赖排序）

1. [ ] claude-proxy.js：新增 OFFICIAL_MODELS 常量表
2. [ ] claude-proxy.js：修复 754-760 透传 model 名（realModelId → modelId）
3. [ ] claude-proxy.js：handleModels 合并官方模型（排最前）
4. [ ] 同步部署：源码 + `~/.mimo2codex/claude-proxy.js` 双处修改（项目铁律），重启 8689
5. [ ] 跑 `node scripts/test-claude-proxy.cjs` + 新增官方模型用例
6. [ ] 按第五节验证清单全量验证
7. [ ] （可选）构建新版本 CC-Gate

## 八、涉及文件

| 文件 | 改动 |
|------|------|
| `claude-proxy.js` | OFFICIAL_MODELS 表 + handleModels 合并 + 透传 model 名修复 |
| `scripts/test-claude-proxy.cjs` | 新增官方模型发现/透传断言 |
| `~/.mimo2codex/claude-proxy.js` | 运行时同步部署 |
| `config_writer.rs` | **无改动**（alias 已带 discovery 环境变量，settings.json 不写 model 字段） |
