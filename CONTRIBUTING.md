# 贡献指南 · Contributing Guide

> 提交代码到本仓库前请通读本文档。  
> This document is the canonical contribution policy. AI agents should also follow it; `AGENTS.md` is a thin pointer to this file plus project-internal conventions.

## 1. 仓库信息 · Repository

| 项 | 值 |
| --- | --- |
| **GitHub URL** | `https://github.com/chenyuxiangg/riscv-sim.git` |
| **SSH URL** | `git@github.com:chenyuxiangg/riscv-sim.git` |
| **默认分支** | `main` |
| **传输协议** | **SSH only**（HTTPS 不支持；本仓库 `git push` 走 SSH 公钥认证） |
| **License** | MIT OR Apache-2.0（见 workspace `Cargo.toml`） |

---

## 2. SSH 密钥配置（首次提交必做 · One-time setup）

### 2.1 检查是否已有 SSH 密钥

```bash
ls -la ~/.ssh/
# 看到 id_ed25519 / id_ed25519.pub 或 id_rsa / id_rsa.pub 说明已有
```

### 2.2 生成新密钥（推荐 ed25519）

```bash
ssh-keygen -t ed25519 -C "your_email@example.com"
# 提示 Enter passphrase（建议设置；空回车 = 无密码）
# 默认保存到 ~/.ssh/id_ed25519
```

### 2.3 启动 ssh-agent 并加载密钥（macOS）

```bash
# 启动 agent
eval "$(ssh-agent -s)"

# macOS 12+：用 keychain 持久化
ssh-add --apple-use-keychain ~/.ssh/id_ed25519

# macOS 11 及更早：
# ssh-add -K ~/.ssh/id_ed25519
```

### 2.4 公钥添加到 GitHub

```bash
# 复制公钥到剪贴板
pbcopy < ~/.ssh/id_ed25519.pub

# 浏览器打开：
open https://github.com/settings/keys
# 点 "New SSH key" → Title 填 "riscv-sim dev" → 粘贴 → Add SSH key
```

### 2.5 验证连接

```bash
ssh -T git@github.com
# 期望输出：Hi chenyuxiangg! You've successfully authenticated...
```

### 2.6 配置本仓库使用 SSH（如果之前是 HTTPS）

```bash
cd /Users/zyzs/lczyzs/code/riscv_sim
git remote set-url origin git@github.com:chenyuxiangg/riscv-sim.git
git remote -v  # 确认两个 URL 都是 git@... 形式
```

### 2.7 （可选）~/.ssh/config 简化

如果同时管多个仓，写在 `~/.ssh/config`：

```
Host github.com
  HostName github.com
  User git
  IdentityFile ~/.ssh/id_ed25519
  AddKeysToAgent yes
  UseKeychain yes
```

---

## 3. Git 基础配置（每个新机器必做）

```bash
git config --global user.name  "Your Name"
git config --global user.email "your_email@example.com"
git config --global init.defaultBranch main
git config --global pull.rebase true   # pull 时 rebase，避免无意义 merge commit
```

**禁止** 在本仓库用 `git config user.email "..."` 设 `noreply` 或匿名 email —— commit 历史要可追溯。

---

## 4. 分支策略 · Branching

- **主分支**：`main` — 永远保持 `cargo test --workspace` 通过；只接受 squash merge
- **功能分支**：`<type>/<scope>` — 例：
  - `feature/m14-bus-ram-pages`
  - `feature/m13-iss-rv32imc-full`
  - `fix/m12-engine-quantum-off-by-one`
  - `docs/progress-v0.4`
  - `chore/ci-github-actions-macos`
- **type 取值**：`feature` / `fix` / `docs` / `chore` / `refactor` / `test` / `ci`（与 commit type 对齐）
- **scope 取值**：模块号 `m01..m23` 或子域 `progress` / `ci` / `wasm` 等
- **命名一律小写、连字符分隔**，禁止大写 / 下划线 / 中文

### 工作流

```bash
# 1. 从 main 拉最新
git checkout main
git pull --rebase origin main

# 2. 切功能分支
git checkout -b feature/m14-bus-ram-pages

# 3. 多次 commit（每个 commit 一个逻辑单元）
git add -p
git commit -m "feat(m14): sparse 4-KiB RAM page table"

# 4. 推 origin（首次）
git push -u origin feature/m14-bus-ram-pages

# 5. 在 GitHub 开 PR：base = main，compare = feature/...
# 6. CI 绿 + 至少 1 个 reviewer 批准 → squash merge
# 7. 删本地分支
git branch -d feature/m14-bus-ram-pages
git push origin --delete feature/m14-bus-ram-pages
```

---

## 5. Commit 格式 · Commit Messages

本仓库遵循 **Conventional Commits 1.0.0**（https://www.conventionalcommits.org/）。

### 5.1 格式

```
<type>(<scope>): <subject>          ← 必填，≤72 字符
<blank line>
<body>                               ← 选填，wrap at 72 chars
<blank line>
<footer>                             ← 选填，BREAKING CHANGE / Refs
```

### 5.2 type 取值

| type | 用途 | 示例 |
| --- | --- | --- |
| `feat` | 新功能 | `feat(m13): add MUL/MULH instructions` |
| `fix` | 修 bug | `fix(m12): quantum off-by-one when event lands on boundary` |
| `docs` | 仅文档 | `docs(progress): bump to v0.4` |
| `refactor` | 不改行为的重构 | `refactor(m18): replace drain swap with proper drain()` |
| `test` | 仅测试 | `test(m13): add RV32I golden vectors from Spike` |
| `perf` | 性能优化 | `perf(m12): use SmallVec for event queue` |
| `build` | 构建/依赖/CI | `build(kernel): add `object` crate for ELF parsing` |
| `ci` | CI 配置 | `ci: add macOS runner with spike + riscv64-elf-gcc` |
| `chore` | 杂项（不动 src/test） | `chore: bump rust toolchain to 1.99` |
| `revert` | 回滚 | `revert: feat(m13) due to wrong ISA encoding` |

### 5.3 subject 规则

- **祈使句**（"add" 不是 "added"，"fix" 不是 "fixed"）
- **首字母不大写**
- **末尾不加点**
- **不写无关前缀**（不要 `feat(m13): Update xxx. Fix xxx.`）
- **长度 ≤ 72 字符**

### 5.4 body 规则

- 说明**为什么**改，而不是改了什么（diff 自己会显示）
- wrap at 72 chars
- 多段落用空行分隔

### 5.5 footer

- 关联 issue：`Refs: #12` / `Closes: #34`
- 重大变更：`BREAKING CHANGE: <description>`

### 5.6 示例

**好**：

```
feat(m14): sparse 4-KiB RAM page table with interval-tree decoder

Replace the flat BTreeMap address decoder with a BTreeMap<PageNum, Page>
plus a sorted Vec<Range> for peripheral windows. Hits the M-14 §4.14
"page-granular RAM" requirement and gives O(log n) decode for the
common 4-KiB-aligned case.

Refs: #42
```

**坏**：

```
fix

改了一些东西
```

---

## 6. PR 流程 · Pull Request

1. **开工前**：先在 GitHub Issues 开 issue（或复用现有）描述要做什么；大改动先在 issue 里讨论方案
2. **开发**：在 feature 分支；多次 commit；每个 commit 一个逻辑单元
3. **本地验证**：
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
   三条全过才推
4. **推分支 + 开 PR**：标题用 conventional commit 格式；正文填「为什么 / 改了什么 / 怎么验 / 截图」
5. **CI 必跑**：GitHub Actions macOS runner + Spike 对拍（v0.3 启用）
6. **Review**：至少 1 个 reviewer 批准；架构级改动（涉及 ADR）需更新 `docs/`
7. **Merge**：squash merge（保持 main 历史干净）；commit message 用 PR 标题

### 6.1 PR 标题约定

PR 标题 = squash 后的 commit 标题 = conventional commit 格式。

### 6.2 模板（PR body）

```markdown
## What
<一句话说改了什么>

## Why
<为什么改；引用 issue>

## How to verify
- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 通过
- [ ] 手动验证：<步骤>

## Risks
<可能的风险 / 向后兼容性 / 性能影响>
```

---

## 7. 提交前自检清单 · Pre-commit Checklist

- [ ] **改动已完成 commit**（不留未提交的工作区改动）；一个 commit 一个逻辑单元
- [ ] `cargo fmt --all` 干净
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 0 warning
- [ ] `cargo test --workspace` 全过
- [ ] 新模块有 `#[cfg(test)]` 单元测试
- [ ] 公共 API 有 `///` 文档（contract crate 严格，kernel 宽松）
- [ ] 没引入 GPL 依赖（合规 R2）；如需 QEMU 走进程隔离
- [ ] 没把 `target/` / `Cargo.lock.bak` / `*.pem` / `.env` 误提交（`.gitignore` 已配）
- [ ] 重大改动（架构 / 跨模块契约）更新了 `docs/` 下对应设计文档

> **为什么有第一条**：代码改完不提交，等于把改动留在工作区——下一次会话、分支切换或并行
> 开发都可能让这批改动丢失或被误收进别的 commit。**开发会话的收尾必须包含一个 commit。**
>
> AI Agent 的完整工作循环见 [`AGENTS.md`](./AGENTS.md) §「工作循环（Agent 必读）」，
> 其中还规定了 `git add` 必须用显式路径、以及 `git push` 需用户明确指示。

---

## 8. 不允许做的事 · Forbidden

- ❌ 直接 commit 到 `main`（必须经 PR + reviewer）
- ❌ `git push --force` 到 `main`
- ❌ 在 commit message 里写 `noreply@github.com` 等匿名邮箱
- ❌ 提交大文件（>1MB 走 Git LFS；本仓库目前未启用 LFS）
- ❌ 提交密钥 / token / `.env`（`.gitignore` 已拦，绕过者责任自负）
- ❌ 引入 QEMU/Unicorn 等 GPL 库到 `riscv_sim_kernel`（R2 合规要求；走进程隔离）

---

## 9. 联系方式 · Contact

- **Owner**: zyzs（GitHub: `chenyuxiangg`）
- **Issue 优先**：bug / feature request 走 GitHub Issues
- **架构级讨论**：开 issue 标 `[arch]` 标签

---

## 10. 附录 · Appendix

- `AGENTS.md` — AI Agent 读的项目约定（精简指针，详细内容见本文件 + 27 篇设计文档）
- `docs/progress/PROGRESS.md` — 项目进度 + 完整开发计划
- `docs/architecture/RISC-V_Architecture_Design.html` — 架构设计（4+1 视图 + ADR）
- `docs/requirements/RISC-V_Requirements_v1.3.html` — 需求规格（13 P0 + 6 层概念空间）
- `docs/module-design-spec/RISC-V_Module_Design_Specification.html` — 23 模块 LLD
- `docs/detailed-design/M-NN_*.html` — 23 篇模块详细设计
