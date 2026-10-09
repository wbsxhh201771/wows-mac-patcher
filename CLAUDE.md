# AI Agent 工作指南

本文档为在本仓库工作的 AI coding agent（Claude Code、Codex 等）提供指令。

## 仓库结构

- `src-tauri/` + `src/` —— Rust + Tauri 2 的 macOS 图形应用（唯一实现）
- `src-tauri/src/patcher/fixture.rs` —— 测试用合成 PE64 DLL（`#[cfg(test)]`，无需 mingw）

## 代码规范

- Conventional commits：`feat:`/`fix:`/`chore:`/`refactor:` 前缀，中文 body
- Commit 原子性：一个 commit 只解决一个语义问题。多模块变更按模块拆分成多个小 commit，单 commit 文件数控制在 10 个以内，每个 commit 可独立 review 和回滚
- 用户可见文案用中文；`patcher/pe.rs` 里的底层 PE 报错保持英文（精确技术诊断）

## 不可偏离的约束

1. 补丁偏移**每次运行从导出表重新解析**，绝不硬编码。硬编码会在游戏更新后写坏 DLL。
2. `state.json`（`~/Library/Application Support/WoWsMacPatcher/state.json`）字段名不改、不新增必填项。改它之前先跑 `cargo test`。
3. `status` 永远只读。`patch` / `clean` 必须经过确认。
4. 需要 root 的操作（`sudo scutil --set HostName`）**不自行提权**，只提供命令与「在终端中打开」。

## 常用命令

```bash
# 测试（最关键的一环，先在 CI/本地跑通再动前端）
cargo test --manifest-path src-tauri/Cargo.toml --lib

# 开发
npm install
npm run tauri dev

# 打包
npm run release:mac        # 产物收集到 out/
```

## 已知陷阱

- `diag::wine_registry_hostname` 会拉起 wineserver，只允许在用户显式点击时调用，禁止放进状态刷新路径。
- 外部盘的 `._*` 扫描很慢，必须走 `spawn_blocking` + 进度事件，不能阻塞 UI 线程。
