# AGENTS.md — Hippocampus 工程协议

本文档定义 `hippocampus/` 目录下智能体工程的默认工作协议与**硬性约束**，用于对齐并吸收 `ironclaw/`、`zeroclaw/`、`codex/` 等项目的成熟经验，但以 Hippocampus 的目标为准。

适用范围：`hippocampus/` 子仓库（该目录自身带 `.git/`）。

---

## 1) 项目目标（Project Snapshot）

Hippocampus 是一个用 Rust 编写的智能体（agent）实现，目标是：

- **可控**：可被外部 harness 稳定驱动，行为可审计、可回放、可限权
- **可扩展**：能力以 trait/模块边界扩展，避免“万能管理器”式耦合
- **可验证**：核心路径可单测/集成测，失败模式明确
- **安全优先**：默认最小权限、默认不外联、默认不执行破坏性动作

非目标（默认）：

- 不追求一次性集成所有模型/工具/记忆后端
- 不在未定义 harness 契约前暴露高危工具（任意 shell、任意网络、任意写文件）

---

## 2) 目录与代码组织（约定）

当前 `hippocampus/` 目录可能还在早期阶段；当引入 Rust 工程后，推荐按以下边界组织（可按需裁剪）：

- `src/agent/`：对话/规划/执行编排（orchestration loop）
- `src/harness/`：与外部 harness 的协议适配（**不要**把策略写进 harness 适配层）
- `src/tools/`：工具接口与实现（文件、搜索、shell 等）
- `src/providers/`：模型/推理提供方（LLM provider / local model / mock）
- `src/policy/`：权限、配额、允许列表、敏感数据治理
- `src/recording/`：回放/记录（request/response/tool-call trace）

硬性规则：

- **策略与适配分离**：`harness` 只做“协议翻译 + 约束执行”，不做“智能体决策”
- **边界清晰**：`tools` 不得绕过 `policy`（例如工具内部直接执行未授权 shell）
- **错误要结构化**：外部 harness 需要可机读的错误码/分类，而不是仅字符串

---

## 3) 工程原则（Normative）

这些原则在 Hippocampus 里是实现约束，不是口号：

- **KISS**：宁可显式分支，也不要隐式魔法
- **YAGNI**：不为“未来可能”增加配置/开关/抽象
- **Fail Fast**：不支持/不安全就明确拒绝（返回结构化错误）
- **Secure by Default**：默认拒绝高危能力（网络、shell、写文件、持久化）
- **可复现**：同输入在同约束下输出应尽量稳定（随机性要可控/可关闭）

---

## 4) Harness 契约与约束（强制）

本节是 Hippocampus 的“安全与可控”地基。**harness 是外部驱动器/执行器**，负责把“用户任务”变成对 agent 的调用，并提供工具执行环境。

### 4.1 术语

- **Agent**：Hippocampus 的决策与编排逻辑（LLM + policies + planners）
- **Harness**：运行 agent 的宿主与约束层（CLI/服务端/评测框架均可）
- **Tool**：agent 请求执行的动作（例如读取文件、运行命令、查询网络）

### 4.2 Harness 的输入（Input Contract）

harness 启动一次“agent run”时，必须提供（至少）：

- **run_id**：唯一 ID（用于追踪与回放）
- **workspace_root**：工作区根路径（所有文件操作必须受其约束）
- **task**：用户任务文本（UTF-8）
- **limits**：资源限制（见 4.5）
- **capabilities**：能力开关/允许列表（见 4.4）
- **environment**：可选环境信息（OS、shell、只读/可写模式等）

要求：

- `workspace_root` 必须是绝对路径
- harness 必须显式声明是否允许网络、是否允许 shell、是否允许写文件
- 不允许“隐式扩大权限”（例如工具失败后自动升级权限重试）

### 4.3 Harness 的输出（Output Contract）

一次 run 的最终输出必须是**可机读的结构化结果**，且包含：

- **status**：`success | failed | cancelled`
- **summary**：对外可读摘要（不含敏感信息）
- **artifacts**：产物列表（修改了哪些文件、生成了哪些文件、运行了哪些命令）
- **metrics**：耗时、工具调用次数、（可选）token 统计
- **trace_ref**：可选引用（本地路径或外部存储 key），用于回放/审计

并且：

- **禁止**把 secrets 写入 summary / artifacts / trace（见 4.6）

### 4.4 能力模型（Capabilities / Allowlist）

harness 必须以 allowlist 的方式暴露能力给 agent。推荐的最小集合：

- `read_file`：只读文件（必须受 workspace_root 限制）
- `list_dir`：列目录（必须受 workspace_root 限制）
- `search_text`：在 workspace 内搜索

高危能力必须默认关闭，并在输入中显式开启：

- `write_file` / `apply_patch`：修改文件
- `run_shell`：执行命令
- `network`：对外网络访问（HTTP/HTTPS/WebSocket 等）
- `process_spawn`：启动子进程（若与 run_shell 分离）

要求：

- 每个能力必须有**细粒度参数约束**（例如 `run_shell` 的命令 allowlist、超时、cwd）
- Agent 只能通过 harness 暴露的工具调用能力，禁止绕过（例如直接打开 socket）

### 4.5 资源与执行限制（Limits）

harness 必须实现并强制执行这些限制（建议默认值写在 harness 配置里）：

- **time_limit_ms**：单次 run 最大时长（例如 10–30 分钟）
- **tool_call_limit**：最大工具调用次数
- **file_write_limit**：最大写文件次数/总字节数（防止失控生成）
- **shell_timeout_ms**：单条命令最大时长
- **shell_cwd_policy**：命令执行目录必须在 `workspace_root` 内
- **memory_budget_mb**：进程内存上限（若可用）
- **output_budget**：日志/trace 最大大小（防止无限打印）

要求：

- 超限要返回结构化错误（例如 `limit_exceeded`），并记录在 artifacts/metrics
- 若 run 因超限中止，status 应为 `failed` 或 `cancelled`，但 trace 必须可审计

### 4.6 敏感数据与隐私（Secrets & PII）

禁止项（必须做到）：

- 不得在日志/trace/产物中写入：API key、token、密码、私有 URL、个人邮箱/电话等
- 不得把 `.env`、密钥文件、凭证文件加入产物或提交建议

要求：

- harness 应对常见敏感模式做脱敏（例如 `Authorization: Bearer ...`）
- agent 侧也应把“可能含敏感信息的内容”标注为不可回显

### 4.7 文件系统边界（Workspace Jail）

硬性规则：

- 所有文件读写必须限制在 `workspace_root` 内
- 禁止访问：`~/.ssh`、系统证书、浏览器配置、全局 git config、Keychain 等
- 禁止跟随 symlink 逃逸 `workspace_root`

建议：

- harness 对路径做 canonicalize，并在每次文件操作前验证前缀属于 `workspace_root`

### 4.8 Shell 执行约束（若启用）

若启用 `run_shell`，必须至少做到：

- **命令 allowlist 或 denylist**（禁止 `rm -rf /`、`dd`、`mkfs`、`shutdown` 等）
- **禁止交互式命令**（需要 TTY 的流程不允许）
- **禁止后台常驻进程**（除非 harness 明确允许并可管理生命周期）
- **网络相关命令**（`curl/wget/nc/ssh` 等）默认禁止，除非 `network=true` 且有目标 allowlist
- 记录：命令、cwd、开始/结束时间、退出码、截断后的 stdout/stderr

### 4.9 网络访问约束（若启用）

若启用网络：

- 默认 **deny-by-default**，以 host allowlist 方式开放
- 必须有请求超时与最大响应体限制
- 记录请求元数据（方法/host/路径/状态码/耗时），不要记录完整敏感响应体

---

## 5) 变更纪律（对智能体开发尤其重要）

- **小步可回滚**：避免“功能 + 重构 + 格式化”混杂
- **风险分级**：涉及 `tools/`、`policy/`、`harness/` 的改动视为高风险
- **默认先写约束再写能力**：新增工具前先定义权限/参数/失败模式

---

## 6) 最小验收（建议）

当 Hippocampus 引入 Rust 工程后，建议把以下作为默认本地验收：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

若某些测试依赖网络或外部服务，应在 harness 契约里明确 “network disabled 时的跳过策略”，避免本地/CI 偶发失败。

