#!/bin/bash
# CC-Gate release script - creates GitHub Release + uploads Mac DMG + Windows exe
set -e

TOKEN=$(gh auth token 2>/dev/null || security find-internet-password -s github.com -w 2>/dev/null)
if [ -z "$TOKEN" ]; then
  echo "GitHub token not found, enter manually:"
  read -s -p "GitHub token: " TOKEN
  echo
fi

REPO="gongminami/cc-gate"
TAG="v0.2.2"
VERSION="0.2.2"
DMG="src-tauri/target/release/bundle/dmg/CC-Gate_0.2.2_x64.dmg"
EXE="/tmp/CC-Gate_0.2.2_x64-setup.exe"
MAC_SHA=$(shasum -a 256 "$DMG" | awk '{print $1}')
WIN_SHA=$(shasum -a 256 "$EXE" | awk '{print $1}')

echo "=== SHA256 ==="
echo "Mac DMG:  $MAC_SHA"
echo "Win exe:  $WIN_SHA"
echo ""

AUTH="Authorization: token $TOKEN"
API="https://api.github.com/repos/$REPO"

# Check if release already exists
echo "Checking existing release..."
EXISTING_ID=$(curl -sS -H "$AUTH" "$API/releases/tags/$TAG" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || echo "")

if [ -n "$EXISTING_ID" ]; then
  echo "Release already exists (id=$EXISTING_ID), fetching upload URL..."
  UPLOAD_URL=$(curl -sS -H "$AUTH" "$API/releases/$EXISTING_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['upload_url'].split('{')[0])")
else
  # Write release body JSON to temp file to avoid shell escaping hell
  BODY_FILE=$(mktemp)
  python3 << PYEOF > "$BODY_FILE"
import json
body = """## Download

| Platform | File | SHA256 |
|----------|------|--------|
| macOS | [CC-Gate_${VERSION}_x64.dmg](https://github.com/${REPO}/releases/download/${TAG}/CC-Gate_${VERSION}_x64.dmg) | \`${MAC_SHA}\` |
| Windows | [CC-Gate_${VERSION}_x64-setup.exe](https://github.com/${REPO}/releases/download/${TAG}/CC-Gate_${VERSION}_x64-setup.exe) | \`${WIN_SHA}\` |

### Verify

\`\`\`bash
# macOS
shasum -a 256 CC-Gate_${VERSION}_x64.dmg
# Windows (PowerShell)
Get-FileHash CC-Gate_${VERSION}_x64-setup.exe -Algorithm SHA256
\`\`\`

### Changes (v0.2.2)

- 修复：Claude Code 后台任务（权限分类器 / 话题检测 / 标题生成）不再乱选模型 —— tier 请求自动跟随当前窗口主模型，冷启动回落 TOKEN_MAP / 内置默认模型
- 新增：统一命令哨兵变量 claude-haiku-follows-main 钉住四个 tier env，后台任务抢跑也不会再打到官方端点报 401
- 测试：claude-proxy 路由测试新增 4 个 tier-follow 用例（共 33 项全过）

### Changes (v0.1.20)

- 新增：支持 Google Gemini 直连 —— 内置 gemini-3-flash-preview / gemini-2.5-pro 模型，走官方 OpenAI 兼容端点（generativelanguage.googleapis.com/v1beta/openai），填 GEMINI_API_KEY 即可在 Claude Code / Codex / Hermes 中选用；中转站新增 Gemini 预设，学生/新机器零配置开箱即用
- 修复：首页切换模型时，复选框 / 路由下拉框变动后「恢复」按钮自动变为「应用」（dirty 检测覆盖路由下拉框与未勾选模型）
- 重构：版本号单一来源，以 Cargo.toml 为准 —— tauri.conf.json 删 version 自动 fallback，发版只改一处；Sidebar 左下角版本号动态读取

### Changes (v0.1.19)

- 新增：deepseek-v4-pro 原生 Responses API 支持（native_responses=true）—— codex-ds 别名直连 api.deepseek.com，绕过 mimo2codex，不再做 Responses→Chat 翻译
- 修正：启动项 mimo2codex 说明文字 —— 明确其仅服务非原生模型（GLM / Qwen / MiMo）经 Codex 时使用
- 测试：更新 bare_aliases 回归测试，断言 codex-ds 直连（不含 8688）、codex-glm 仍走代理

### Changes (v0.1.18)

- 新增：OpenCode 配置写入（write_opencode_config）—— 自动写 ~/.config/opencode/opencode.jsonc 的 ccgate provider（chat-proxy 8690），默认模型指向 ccgate
- 修复：OpenClaw / OpenCode 代理状态检测假阴性 —— is_agent_proxied 改为按实际写入格式检测（models.providers.ccgate / provider.ccgate），UI 不再误报"未代理"
- 修复：codex config 重写保留用户 [projects.*]（信任目录）与 [mcp_servers.*]（MCP 服务器）段，不再整文件覆盖清空
- 修复：JSONC/JSON5 解析改用手写 lenient 剥离器 → 官方 json5 crate（行级剥离会误删必需分隔逗号）
- 新增回归测试：preserve_user_sections / jsonc_lenient / full_apply_all_agents_proxied（gated，9 agent 全 proxied 断言）

### Changes (v0.1.17)

- 修复：codex 模型目录文件名撞车根治 —— \`cc-switch-model-catalog.json\` → \`cc-gate-model-catalog.json\`（paths.rs / config_writer.rs / DESIGN.md），与 CC Switch 彻底隔离，不再互相覆盖
- 新增：合并模型目录生成逻辑验证（codex_cli ∪ codex_desktop 全模型进入切换器）
- 测试：全链 headless 回归（应用 → 恢复 → 再应用），Aider/Cursor 别名块恢复生效
- 已知问题：OpenClaw 代理状态检测假阴性（is_agent_proxied 格式不匹配）；OpenCode 配置写入未实现（待下版）

### Changes (v0.1.16)

- 无后缀 alias（codex / claude / aider）= 官方原生调用（codex→官方 OpenAI gpt-5.5，claude→api.anthropic.com，aider→裸命令）；带后缀 alias 仍走本地代理
- 移除非线智能中转（代码/注释/测试全清），providers.json 仅剩 4 个官方直连 provider（DeepSeek / GLM / Qwen / MiMo）
- 快速添加中转弹窗预设只留 OpenRouter
- Shell 集成页别名列表新增原生条目展示
- 代理层修复：count_tokens 端点 + isAnthropicNative 检测
- 新增回归测试（原生/代理 alias 生成断言）与无 GUI 应用配置工具
"""
print(json.dumps({"tag_name": "${TAG}", "name": "CC-Gate ${TAG}", "body": body, "draft": False}))
PYEOF

  echo "Creating release..."
  RELEASE_RESP=$(curl -sS -X POST -H "$AUTH" -H "Content-Type: application/json" --data-binary "@$BODY_FILE" "$API/releases")
  rm -f "$BODY_FILE"

  HTML_URL=$(echo "$RELEASE_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('html_url',''))")
  UPLOAD_URL=$(echo "$RELEASE_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['upload_url'].split('{')[0])")
  echo "Release created: $HTML_URL"
fi

echo ""

# Upload Mac DMG
echo "Uploading Mac DMG ($(ls -lh "$DMG" | awk '{print $5}'))..."
curl -sS -X POST -H "$AUTH" -H "Content-Type: application/octet-stream" \
  --data-binary "@$DMG" \
  "$UPLOAD_URL?name=CC-Gate_${VERSION}_x64.dmg" > /dev/null
echo "  OK"

# Upload Windows setup exe
echo "Uploading Windows setup exe ($(ls -lh "$EXE" | awk '{print $5}'))..."
curl -sS -X POST -H "$AUTH" -H "Content-Type: application/octet-stream" \
  --data-binary "@$EXE" \
  "$UPLOAD_URL?name=CC-Gate_${VERSION}_x64-setup.exe" > /dev/null
echo "  OK"

echo ""
echo "Done: https://github.com/$REPO/releases/tag/$TAG"
echo ""
echo "=== SHA256 (for reference) ==="
echo "Mac:  $MAC_SHA"
echo "Win:  $WIN_SHA"
