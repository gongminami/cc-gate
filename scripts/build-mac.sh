#!/bin/bash
# CC-Gate macOS 构建 — 必须用这个脚本，别直接跑 tauri build！
# 原因：dist 变化不会触发 Tauri 重新嵌入资源（codegen 缓存坑），导致
# "改了半天装出来还是旧界面"。touch 源文件强制 lib 重编 + 宏重读 dist。
set -e
cd "$(dirname "$0")/.."
npm run build
touch src-tauri/src/lib.rs src-tauri/src/main.rs src-tauri/build.rs src-tauri/src/config_writer.rs
npm run tauri -- build "$@"
# 产物自检：新前端 hash 必须出现在二进制里（抽查最新 js 文件名）
JS=$(ls -t dist/assets/index-*.js | head -1 | xargs basename)
if grep -aq "$JS" src-tauri/target/release/cc-gate; then
  echo "✅ 前端已嵌入 ($JS)"
else
  echo "❌ 警告：最新前端 $JS 未嵌入二进制！" && exit 1
fi
