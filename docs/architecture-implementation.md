# 架构整理实施记录

**更新日期：** 2026-11-27（北京时间；补充分支提交说明）

**规范：** [DZC-ARCH-001 v1.2 Final](specs/architecture-simplification.md)

**源码基线：** `1f1641a0455bfcdbc72b4ba70a24d3cb810887a9`，`0.2.0-beta.4`

**工作分支：** `refactor/architecture-phase1`

本页记录规范交付后的实际实施情况。规范中的现状统计、历史验证和发布分析仍对应冻结基线；本页的“已实现”“已检查”“未执行”分别描述代码、验证和剩余工作。第一批实现与验证记录完成于 2026-11-20；2026-11-27 按用户要求整理为提交，目标远端分支为 `refactor/architecture-phase1`。本批保留上述源码基线，合入后续主线前仍须处理适配并完成第 5 节验收。

## 1. 第一批范围与状态

| 任务 | 本批交付 | 当前状态与剩余门槛 |
|---|---|---|
| R0 | 将最终规范归档到仓库；校准 [AGENTS.md](../AGENTS.md) 的目录、依赖、运行入口、平台范围和构建说明 | 基线归档及文档校准完成；本轮未重算全仓库行数，冻结统计见规范第 2 章 |
| C0 | 重写 [CI 基线](ci.md)，核对真实触发、测试入口、六个 NSIS 场景、36 项发布资产和名称消费者 | 本地配置与文档核对完成；外部保护设置、当前完整运行耗时和成功率未核实，不能将 C0 全部验收标为完成 |
| R1 | 主题 Core API 返回事务 outcome；Tauri 主题命令使用共享 HistoryService；约束主题库提交及事件发布顺序 | 实现与两轮静态复核完成；新增 Rust 测试未运行，编译、客户端和原生验收待完成 |
| 其余任务 | R2–R8、C1–C7、M0–M4 按规范保留 | 本批未进入；CI/Release 行为及 macOS 产品支持没有因本批文档调整而改变 |

本批没有修改 workflow、Cargo/pnpm manifests 或 lockfile、协议 schema、数据库 migrations、设置键和 release profile。未进行远程 CI、安装、签名或发布操作。

## 2. R1：已实现的主题边界

### 2.1 Core 对外接口

[主题 API](../crates/dzc-core/src/api/themes.rs) 新增以下公开能力，两个 outcome 从 [api 模块](../crates/dzc-core/src/api/mod.rs) 导出：

| API / 类型 | 用途与语义 |
|---|---|
| `HistoryService::custom_themes()` | 读取完整自定义主题库 |
| `HistoryService::import_theme(ThemePackage)` | 返回 `ThemeImportOutcome { theme, themes }`；主题包含校验、规范化后的保存值，列表来自该次已提交事务 |
| `HistoryService::remove_theme_with_outcome(id)` | 返回 `ThemeRemovalOutcome { themes, active_reset }`；删除活动主题时，主题库和 `app.theme=mica` 仍在同一事务提交 |
| `HistoryService::effective_base_theme(theme)` | 非 `custom:` 值原样返回；自定义主题不存在、读取失败或内容损坏时返回 `mica`，不写入设置 |

原有 `import_theme_file()` 仍返回单个 `ThemePackage`；原有 `remove_theme()` 仍返回 `bool`。这两个接口投影新的 outcome，保留已有 CLI/Host 消费者的调用契约。

[Repository](../crates/dzc-core/src/infrastructure/repository/custom_themes.rs) 保留原来的事务、验证和回滚逻辑，将原先被丢弃的完整列表向上传递。API 不在提交后另行读取列表，因此事件使用的是本次事务结果。

### 2.2 Tauri 消费者迁移

[theme_cmd.rs](../src-tauri/src/app/commands/theme_cmd.rs) 的列表、导入、删除、导出和有效 base 查询统一使用 `DbState.history_service()`。该文件不再直接使用 repository、原始连接或 `HistoryService::from_legacy_connection`。初始化和其它领域的 bridge 保留到 R6，未改变 GUI 初始化策略或 RAM session。

新增应用级 `ThemePublicationState`，由 [setup.rs](../src-tauri/src/app/setup.rs) 注册为 Tauri managed state。导入和删除按以下顺序执行：

1. 导入时，文件读取和解析在阻塞任务内完成，随后才取得发布锁。
2. 发布锁覆盖 Core mutation 和 `custom-themes-changed` 完整列表事件；Core 返回时数据库锁已经释放。
3. `commit_and_publish` 返回时释放发布锁。
4. 删除活动主题时，随后沿原路径执行 `set_theme("mica")`；成功后再发 `settings-changed`。

发布锁不跨 `.await`，也不覆盖原生窗口外观操作。它只协调该 GUI 通过此入口进行的主题库修改，不声称串行化其它 Host 客户端或所有设置写入。事件发送失败仍按原有行为处理，不把已经提交的操作改成可重试的失败。

Tauri 显式映射为原有 `AppError::Validation`，保留错误类别和显示文案。命令名、前端传入参数和事件 payload 不变；新增 `State` 由 Tauri 注入。启动时缺少主题设置的 `retro` 默认值也保持原状，与自定义主题异常时的 `mica` fallback 分开。

### 2.3 测试代码

新增 3 个 Core inline tests，覆盖完整事务快照、规范化结果、旧文件导入返回值、失败不改主题库/选择、活动删除 fallback、旧 bool 接口及损坏库的有效 base 回退。现有 2 个 repository tests 加强完整列表断言；已有事务失败触发器继续覆盖主题库和 fallback 回滚。

新增 2 个 Tauri inline tests，使用真实 Core 内存库和标准库线程/channel：

- `theme_publication_preserves_commit_order_while_a_publisher_is_paused`：暂停首次事件发布，确认数据库可以读取、发布锁仍阻止后续 GUI 提交，并检查发布顺序。
- `theme_publication_does_not_emit_failed_mutations_or_change_error_kind`：失败不发事件、不改变错误类别/文案，随后有效操作仍可完成。

没有增加 Rust dev-dependencies 或测试框架。这些 Rust 测试尚未执行，静态复核不能代替编译和运行结果。

## 3. C0：已修正的 CI 文档

[docs/ci.md](ci.md) 现在按实际配置区分日常、手动 full 和正常 tag 发布，明确以下事项：

- 日常 7 个基础 job，加按路径选择的历史 MyGo installer；手动默认 full 展开为 24 个实例。
- 正常 tag 发布不调用完整源码 CI，也不发布 Flatpak；当前精确集合是 36 个资产。
- 历史/合成 NSIS fixture、当前最终包、纯逻辑 fixture、真实性能和原生执行是不同证据。
- Windows ARM64 当前是交叉构建及静态检查，不能记为原生执行成功。
- CLI 环境变量缺失、原生测试直接 return、未接入的 Host/Wayland 验收、当前候选和恢复链仍有缺口。
- `source-acceptance`、36+1 资产规则、冻结候选和恢复状态机属于 C1–C7 后续实施；本批没有将它们接入发布。

job 显示名称、artifact 前缀、旧恢复流程消费者及外部保护配置的未知项已列出，供后续改 workflow 时同步处理。

## 4. 本批实际验证

以下命令在 2026-11-20 的实施工作区运行，均从仓库根目录执行；不沿用规范第 13 章的历史测试计数。2026-11-27 提交前只复核改动范围及补丁空白，未重复运行产品测试。

| 检查 | 实际结果 | 能证明的范围 |
|---|---|---|
| `node --test scripts/tests/packaging-sidecars.node.mjs` | 24 项：23 通过、0 失败、1 跳过 | 打包/sidecar 契约的 fixture 检查；不是真实最终安装包验收 |
| `python3 -B -m unittest discover -s scripts/tests -p test_privacy_window_permissions.py -v` | 1 项通过 | 现有隐私窗口权限静态检查 |
| `python3 -B scripts/generate-protocol.py --check` | exit 0 | 当前协议生成一致性 |
| `git diff --check` | 通过 | 已跟踪改动的补丁空白检查 |
| 文档链接及改动范围检查 | 31 个相对链接有效，9 个预期文件有变更，无额外文件 | 文档可定位；新文档无行尾空白；本批修改范围一致 |
| API 调用点、State 注册、事务结果、锁/借用边界静态复核 | 未发现阻断项 | 代码审查结论，不是 Rust 编译或运行结论 |

跳过的 Node 用例为 `native acceptance refuses local execution before any product operation`，其条件是 `process.platform !== "win32"`；当前 Linux 环境未执行该 Windows 用例。不能把 24 项全部记为通过。

本环境可用 Python 3.12.14、Node 24.22.3；未找到 `cargo`、`rustc`、`rustup`，也没有项目 `node_modules`。因此本批没有运行 Rust 编译/测试、前端测试/构建、Tauri 原生窗口或真实安装/发布验收。

## 5. R1 剩余验收

在具备仓库要求的 Rust 工具链、桌面系统依赖及既有构建输入的环境中，先执行：

```bash
cargo test --locked -p dzc-core --lib
cargo test --locked -p dezirclip --no-default-features --features desktop --bin dezirclip app::commands::theme_cmd::tests -- --nocapture
```

必须在输出中确认 3 个新增 Core 主题测试、2 个原有 repository 主题测试和 2 个 Tauri 发布测试实际运行。Tauri 过滤命令应匹配并执行 2 个测试；零匹配、skip、early return 或仅 `cargo check` 均不能代替运行验收。现有平台所需的其它测试继续按 [CI 基线](ci.md) 和规范执行。

随后在当前支持的 Windows/Linux 客户端检查：主题列表、合法/非法/重复导入、导出内容、删除非活动和活动主题、失败时数据及选择不变，以及连续/并发操作的完整列表事件顺序。活动删除还需观察实际窗口外观与 `theme-changed` / `settings-changed` 顺序。此项不能仅用内存库的 publication helper 测试替代。

上述检查通过并记录实际 source revision、feature/profile、平台和结果后，才可将 R1 标为完成。当前不具备宣称性能改善、编译通过或原生行为等价的运行证据。

## 6. 维护量、回退与后续顺序

本批 5 个 Rust 文件相对固定基线净增 283 个物理行；按各文件首个 inline test module 边界计，55 行在测试模块前，228 行位于测试模块中，包含空行和注释。这是新增 outcome、发布顺序保护和回归测试的成本，不是产品 SLOC 或性能测量。收益是主题消费者退出数据库内部访问，不能以本批净行数宣称仓库已缩小。

没有数据库、协议或设置格式迁移。回退本批代码不需要转换用户数据；提交前可按本页列出的文件撤回本批改动，提交后用对应变更提交的 revert，并保留用户之后产生的独立修改。无需删除数据库或重置配置。

下一步先完成 R1 运行验收；随后按规范推进 R3 路径/依赖闭包以及 C1/C2 的真实测试状态与执行范围。R4 CLI/Service 拆分须等 R3 和打包契约准备完成。Swift/macOS 继续以 M0 的真实系统操作切片为起点，本批没有增加空平台实现或新的发行承诺。

## 7. 在 v0.2.0 上继续实施

接手以正式版 `ba0ad71f7b9d8caddac45d0bc5588254a8273482` 为产品基线，保留随后独立验证的 X11 更新检查、快速粘贴布局和 Arch 打包改动。规范继续说明原始设计与冻结基线，本节记录当前迁移和证据。

- **R1 主题边界**。迁移 Core 事务结果和 Tauri 的发布顺序保护，兼容原 CLI/Host 接口。Windows Rust 1.99.1 下 Core 单元测试 483 项通过。Windows 和 Linux Rust 1.99.1 下 `desktop` debug 二进制的两项发布顺序测试分别通过。
- **R1 原生窗口验收**。Windows 11 WebView2 和 Linux X11 WebKitGTK 使用 `ff90ce4b` 的本地 debug 桌面、生产 React bundle 和临时 profile，通过实际 Tauri bridge 运行同一场景。两次并发有效导入发布按提交顺序排列的完整主题列表。非法与重复导入被拒绝，库和当前选择保持。导出 JSON 使用预期 ID 和原 CSS。删除非活动主题后活动选择保持；删除活动主题时依序观察到主题库事件、`theme-changed` 与 `settings-changed`，并确认原生窗口内 React 的 `theme-custom` 切回 `theme-mica`、数据库保存 Mica。Windows 使用隔离的可执行文件旁数据目录和 WebView2 配置。
- **R3 路径闭包**。早期 WebView 探针和 CLI 共用 Core 的重定向和 profile 选择算法，保留空 portable 目录、当前重定向与旧身份目录并存、普通文件目标三处差异。CLI 直接使用公开路径 API。仍需验证只读 GPU 探针的 SQLite/WAL 行为，并迁出其余 CLI 跨入口依赖。
- **C0/C1**。CI 文档已按 v0.2.0 和 Arch 发布分支校准。Windows ConPTY 入口明确构建当前 CLI，零测试和 skip 会失败。可运行的桌面矩阵使用同一份实际 CLI/Service 执行 Host 客户端、BuildInfo 和 Schema 24 实际写入与种子数据测试。后面三项在 Windows 和 Linux 本地各通过一次。Linux Tauri 套件在 Xvfb 中运行，修复截图边界测试缺少显示器的问题，本地为 270 通过、3 项明确 ignored。其余原生 opt-in 和发布源准入继续按规范推进。
- **C2 桌面专项矩阵**。按请求选择完整矩阵、Linux x64、Windows x64 或两者的并集。仅请求 Windows 性能比较时会选择其 Windows x64 前置验收。独立诊断入口沿用既有规则，完整的场景计划与结果汇总仍待接入。
- **R3 更新策略归属**。Host 的离线模块为 Tauri 更新和 MyGo 通过 CLI 的索引检查提供同一份版本、平台、文件名和签名元数据规则。网络、安装和退出协调仍由对应桌面入口处理。
- **R3 CLI 依赖**。启动项、CLI PATH、技能和目录定位实现归入已有 Platform crate，CLI 和 Tauri 直接使用。CLI 的主题、历史 DTO、受保护标签、密文格式识别和有界读取直接走 Core 的窄入口。仍有备份/附件、测试数据库、日志入口和 CLI 专属 terminal/OSC52 模块需迁移。Windows 与 Linux PATH 测试分别通过 6 和 7 项，Windows 登录项测试和桌面类型检查通过。
- **R3 CLI 旧数据库接口**。附件读取走 Core，原生诊断直接写标准错误。MCP 与备份测试创建隔离的旧版 SQLite 文件，再由和实际 CLI 相同的公开 API 打开。保留密文排除、分页、预览边界、图片原始字节和备份失败时旧文件保护的覆盖。Windows 和 Linux 分别通过 90 项 CLI 测试。CLI 的 terminal/OSC52 源文件会随 R4 一起移动。
- **R2b 字符目录**。React 直接读取 `assets/catalogs/emoji.json`，Go 独立构建时内嵌自己目录中的生成副本。CI、MyGo 打包逐字节计算 SHA-256，现有内容及顺序保持。Tauri 和 MyGo Web 前端生产构建、Tauri 打包目录搜索、31 项字符单元测试和 Linux Go 桌面包测试已通过。主题目录仍待迁移。
- **R4 CLI 与 Service 的包边界**。两个可执行文件分别由 `dzc-cli` 和 `dzc-service` 生产。Tauri 的打包配置通过 `externalBin` 各登记一份，独立 CLI 也使用自己的 Cargo 包与既有 ABI 构建目录。Windows x64 NSIS 和 Linux x64 deb 本地 debug 包各解出一份 CLI 和 Service，SHA-256 与暂存记录一致，进程由解包后的 CLI 找到旁边的 Service，实际 CLI/MCP 验收通过。Windows 两个 helper 的图标资源也已读取验证。Windows 与 Linux 的独立 CLI/Service 套件通过，实际 Service 的无 WebView 验收两边各执行一次。待 GitHub 复核交叉构建和后续正式包流程。
- **C0 版本与性能 fixture**。版本脚本在独立 fixture 中只需要版本解析与需要更新的文件，测试从 Cargo 清单获取完整工作区成员。Windows 性能 fixture 校准了受审查的输入清单哈希。Linux 本地包装测试运行 427 项，16 项在本地环境按既有条件跳过。
- **R5a Profile 入口**。Go 的 profile 查询和打开数据目录使用 application 包，宿主的 Shell 调用通过函数传入。同名绑定的签名及 53 项公开方法保持。Linux Go 模块测试通过，React Library 的集成用例通过实际 Rust Service 读取 profile，并与临时数据目录核对。其余控制器和 WebView 页面验收随 R5 继续。
- **R2a 更新偏好目录**。自动检查与更新通道的设置标识、标签和通道选项由 `protocol/settings.catalog.json` 生成至 Rust、Go、React 和 wire schema。Core 显式保留 view mutation 与隐私 epoch 检查，两项在空库均不预写，旧配置字符串读取与按已安装版本选择通道照旧。Windows/Linux Core 回归、Rust/Go wire fixture、Go 桌面测试与 React 类型和更新策略测试通过。
- **R5b 类型化 Profile 与事件入口**。应用通过明确的 profile 能力读取并打开数据目录，两种宿主适配器保留各自原有调用。MyGo 的事件和隐私状态中使用的 wire 类型由协议生成，现有运行时权限校验保留。Tauri 和 MyGo Web 生产构建、Tauri WebView 启动冒烟、Linux Go 桌面测试及三语更新文案生成检查通过。
- **R6 自定义背景**。Tauri 查询背景改走共享 HistoryService，保留文件读取前后两次选择检查。Windows 和 Linux 测试用公开 Core 设置 API 写入实际 PNG 路径，核对读取字节，并验证切换选择后的旧资源请求会被拒绝。
- **R2b 内置主题目录**。`design/themes.json` 登记原有六种主题的顺序、标签、配套 CSS 入口、默认主题及 Windows 原生材质。Core 与 MyGo 使用生成值，React 读取目录，构建校对固定 Deziro UI 的主题 ID、CSS 和三语标签。CSS、采样 presets 和许可证仍由原组件包提供。Windows/Linux 各通过八项 Core 主题测试，Windows 三项材质计划、Linux Go 模块及 React 主题包检查通过。
- **R3 早期 GPU 探针**。启动时先尝试取得 SQLite 主文件现有锁，Core 仅查询已检查点的只读数据库，不初始化或恢复数据库。活跃写入、未清理的 journal/WAL/SHM 或不支持锁时保留 false fallback。Linux 取得排他锁需要可写描述符，探针本身没有写操作。Windows/Linux 各通过三项测试，覆盖缺库、无效库、旧库、正常关闭的 WAL、并发 writer 和原有布尔值。
- **C4 WebView 验收测试**。Windows 原生升级验收调用独立的 CDP 页面查找函数。现有 fake clock 用例现在直接调用同一个函数，覆盖拒绝、超时、预算用尽、到期后响应和正常响应。移除 Python 读取源码并用 `node -e` 间接运行的入口，Windows 本地成功执行七项 Node 用例和八项其余 Python 检查。
- **R6 快贴和更新设置**。Tauri 从同一次 Core 读取取得列表/环形布局及扇区数量，保留旧设置值的回退规则。自动检查仅在明确开启后调度，原有通道偏好和分通道时间戳由窄 Core API 访问。Windows/Linux 桌面类型检查与更新设置回归通过，Windows 验证了快贴读取缺省值和无效旧值。

### 启动设置与候选产物

- Tauri 通过 Core 的桌面启动接口装配原有数据库、仓库与缓存，并通过类型化设置读取窗口尺寸、置顶和启动偏好。Windows 和 Linux 的实际 SQLite 测试验证原有 WAL、默认值、旧配置、保存窗口大小及游戏模式回退。尚有其它命令使用 legacy bridge。
- 两端 React Web 构建共用隐私状态。MyGo 兼容桥直接读取会话状态，避免桥接与会话生命周期之间的循环依赖。TypeScript 和 18 项隐私测试通过。
- 四个独立 CLI 归档和桌面包按 artifact ID 下载，绑定固定 draft 并按远端 asset ID 读回。共享 NSIS 样件的六个 Windows 场景已通过 GitHub Actions，其中 LegacyUninstall 继续使用固定的历史公开安装器。完整出货包验收、发布封存与中断恢复仍待完成。
- 快贴布局的键、可选值和标签进入共享设置目录，Core、MyGo 和 React 使用相同的列表。Host 实际写入/读回、Go 测试与 React 类型检查通过。
- MyGo Native 将设置保存与显示会话分开。更新通道、开关、窗口置顶和卡片固定可就地更新，表情页签保留导航并为内容区添加过渡。Linux X11 以实际程序和隔离 profile 验证了这四类操作，Windows/Linux 的 Go 套件通过。

- **R6 旧桌面仓库边界**。Tauri 的数据路径迁移、快贴条目回查、表情资源写入、设置重置和旧敏感记录修复统一走现有 Core API。Linux 验证 Core 492 项测试与 Tauri 测试目标类型检查。配置重置失败会回滚，敏感历史修复可安全重试。
- **C5 独立 Windows 最终包验收**。一次只读重放同时覆盖已发布签名 Tauri 和 MyGo 的 v0.2.0-rc5 → v0.2.0。验收检查旧设置和历史记录、三个实际安装文件的 SHA-256、启动、重装及卸载。发布器将两个验收报告和该次打包产物绑定到固定 draft。此处证据范围为 Windows x64 和该升级组合。 [Windows 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46040562056)
- **Darwin 预备验收**。macOS 26 ARM64 runner 用临时签名的 Swift 程序启动实际 Rust helper。Swift 在发送首帧前核对有效 UID、内核 PID、audit token 对应的运行签名和预期二进制身份。helper 核对启动者 PID，持有独占采集租约并在主线程写入系统剪贴板。Swift 从另一进程读回同一文本。helper 在隔离的 Keychain 写入、重开、读回和删除试验凭据。Core 的 FIFO、符号链接和路径替换测试也通过。原型使用已有帧格式与合成响应。生产 Host Desktop grant、主线程调度、发行签名和常驻权限尚未验收。 [Darwin 实测](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46047626597)
- **R6 快贴显示与粘贴设置**。Tauri 的快贴辅助窗口通过 Core 的一次读取取得语言、配色模式、透明度和基础主题。自定义主题只在选中时读取。粘贴热路径改用现有队列配置 API。Linux 通过 9 项配置回归测试和 Tauri 测试目标的类型检查。

- **C5 Linux 最终包运行**。使用 GitHub 发布资产 ID、摘要、Tauri 签名和固定的历史 RPM 摘要读取 v0.2.0。Linux x64 本地隔离 X11 会话实际启动 Tauri 和 MyGo 的 AppImage，以及两种 DEB/RPM 所带的启动入口。六个包都显示了独立 CLI 写入的测试历史，随包 CLI 和 Service 的 BuildInfo 通过核对，并确认了实际运行的程序哈希。AppImage 使用其原生解包运行入口。GitHub 托管 x64 和 ARM64 实例也分别通过六个包的同一套检查。Tag 发布现在按不可变 artifact ID 收集本次候选报告，并在公开发布和更新通道同步前逐一核对所有 Linux 二进制包及其成员哈希。报告校验通过合成发布记录覆盖缺包、摘要、来源和测试失败。实际 tag 路径、Arch 原生安装、DEB/RPM 安装、重装和卸载仍待验证。 [Linux 双架构验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46103511034)

- **R6 Tauri 共享装配**。删除旧的 `DbState` 适配器。Tauri 在启动时按原有桌面 SQLite 初始化政策创建一个 `HistoryService`，各命令和 Embedded Host 复用同一组仓库、缓存和既有 RAM session。设置读写和桌面路径选择走 Core，保留旧值默认规则、数据库错误文案和窗口侧效果。Linux Core 单测默认配置 493 项通过，含现有桌面适配配置 494 项通过。Linux X11 Tauri 测试 272 项通过，3 项按原规则忽略。Windows x64 编译了桌面测试目标。独立的备份格式和部分存储测试仍使用原 legacy 导出。

- **R6 Tauri 退出旧 Core 兼容层**。Tauri 的产品入口和测试使用公开 Core API，备份检查继续支持既有文件格式入口。搜索、队列、备份和历史测试保留真实存储和权限边界，Windows 迁移使用旧版 SQLite 文件中的真实 DPAPI 字段。Linux Core 单测 496 项通过，Linux X11 Tauri 269 项通过、3 项忽略，备份集成 9 项通过。Windows 的 DPAPI 迁移回归通过。无兼容特性的独立 Platform IPC 样件也已在 Linux 编译通过。

- **R6/R7a Core 兼容出口退役**。删除现行 `legacy-bridge`、仓库重导出与仅供桥接使用的方法。捕获性能适配器使用公开 `HistoryService`、`CaptureStore` 和类型化设置，独立 SQLite 连接只验证合成数据。Windows 分别以旧单包基线 `802808b` 和拆分后的 `90ba9960` 运行三个真实适配器，各核对六种新建、去重和加密转换场景。拆分基线的复验使用已删除兼容层的当前 Core。Linux Core 496 项通过，独立 IPC 样件编译通过。功能验收未采集性能样本。

- **C7 历史发布入口退役**。远端确认 alpha.2 的原 release 早已公开，alpha.9 的 MyGo 安装器也已具备 GitHub 资产摘要。删除两项手动菜单及对应 jobs，保留历史验收材料和最新版的 tag、双桌面恢复及更新通道入口。YAML 校验覆盖 11 个发布任务的依赖，打包与 updater 两套 Node 测试 146 项通过。 [alpha.2](https://github.com/qiyuanhuakai/dezirclip/releases/tag/v0.2.0-alpha.2) [alpha.9](https://github.com/qiyuanhuakai/dezirclip/releases/tag/v0.2.0-alpha.9)

- **C6 双桌面更新通道恢复**。首次创建草稿时写入候选归属，staging 名绑定候选、manifest 内容和旧 active 的资产 ID 与哈希。恢复可复用原 staging 或已激活的同一资产，拒绝归属未知的草稿、staging 及缺失 active 的未识别状态。在离线 GitHub API 模型中分别中断产品公开、草稿创建、上传、备份改名、激活和草稿公开，原 run 的成功前置仍受既有检查。126 项 updater 测试通过。取消的原 run 准入和独立候选封存仍待接入。

- **M2 Swift 握手与帧约束**。Swift package 从现有 JSON Schema 生成握手 DTO 和传输上限。macOS 26 ARM64 托管运行通过有界读写、取消、嵌套、非法权限、整数边界以及缺失和 null 的区分。签名 Swift helper 在核对实际 Rust 进程身份后使用这套握手，并验证实例和权限。Host 响应来自原型。历史、订阅、snapshot 和生产 grant 仍在后续范围。 [Darwin 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46111989195)
- **C5/C6 候选冻结与发布恢复**。正常发布先在独立 job 封存 40 项产品和固定名称的候选附件。附件绑定原 release ID、原源码验收及构建/验收/上传 artifact、各产品上传截止时间和待发布通道摘要。公开任务核对成功封存 job 的资产 ID 与 hash。恢复任务从原成功封存 job 按 ID 下载凭据，在全部原前置均成功的情况下可继续 publisher 取消后的发布。离线模拟验证上传响应丢失与原字节复用、错误 producer、额外附件、不同截止时间及取消门槛。独立复核被取消的封存 job 与真实 tag 链仍待验证。

- **M1 Darwin Keychain**。签名 Rust helper 使用显式选定的隔离文件型 Keychain，为新 profile 创建密钥。普通读写只读取既有密钥。Core 保存并重新读取真实 SQLite 敏感条目，核对授权前后的可见性。Keychain 锁定后，已保护条目的读取会报告不可用，新的敏感写入会失败。版本化 Keychain 密文和既有 Linux AES-GCM 共享 codec，Linux 格式以固定向量核对。Windows DPAPI 的既有格式保持。发行 helper 的 Keychain 访问控制和 profile ID 持久化仍需实测。 [Darwin 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46114265742) [Apple Keychain 实现说明](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains)
- **R6 Host 原生导出撤销边界**。将进程级原生文件撤销交给 Service 和 Tauri 的适配器显式实现。合成 Host 各自注入撤销行为，避免并行测试中的一次隐私切换删除另一个测试的临时附件。Linux 三轮 Host 套件均通过各 87 项，原有独立 Go 客户端用例仍按条件跳过。Linux Core 498 项和 Platform 183 项通过，Tauri 测试目标通过类型检查，Windows Host 与 Platform 测试目标通过类型检查。

- **M1 Darwin 共享 Host 路由**。macOS 26 ARM64 在真实 Unix socket 上运行与 Windows、Linux 相同的 Host、Core 和帧协议。验收写入实际 SQLite 历史，读取事件及正文快照，拒绝无授权的 Desktop 角色和另一条已授权连接读取原快照，验证复制请求到达注入的剪贴板适配器，并核对撤销后的连接权限。窗口粘贴、字体枚举、快捷键等平台操作按目标开放。Swift 已单独验证签名 helper 和系统剪贴板，Swift 连接真实 Host 的签名启动链仍待验证。Linux Host 88 项通过，1 项需预构建 Go 客户端。Windows 本地 84 项通过，4 项需要独占采集所有权的用例因现有桌面进程占用而未纳入复核。 [Darwin 共享 Host 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46117245223)

- **M2 Swift 历史与事件 DTO**。从同一套 Schema 生成历史页、事件、快照、复制参数和错误类型。macOS 专项对提交 `093aaa7a` 跑完 47 项适用于 Darwin 的 Host 测试，另有 1 项需要预构建 Go 客户端。Swift 的 11 项测试覆盖共享整数和长度上限、未知字段、缺失和 null 的区分。Signed helper 的剪贴板、Keychain 和部分读取取消验收也通过。Swift 会话、同连接快照组装和签名 Swift 客户端访问实际 Host 的验收仍待接入。 [Darwin 专项验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46117837911)

- **M2 Swift 会话与真实 Host**。提交 `397ba3e6` 的 macOS 26 ARM64 专项通过 15 项 Swift 测试和 47 项 Host 测试。临时签名的 Swift 探针从继承管道取得短期 Desktop grant，核对实际 helper 的代码签名，同一用户的其他进程无法通过父进程限制。隔离 SQLite profile 中的实际历史通过内联快照和多分块快照读回。事件长轮询在独立连接取消并重连，复制请求在该轮询期间写入并从系统剪贴板读回。旧隐私 epoch 和已撤销的 grant 被拒绝，关闭继承管道会排空 Host。单测还验证响应关联、分块界限、重新同步信号和复制回执丢失。两个 Rust 样件复用一次 Cargo 构建。发行 helper 的启动配置、签名权限和持久 profile 仍需验收。 [Darwin 端到端验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46119015783)

- **M1 Darwin 持久 profile**。提交 `655fd501` 把私有目录、独占写入锁、SQLite 身份和 Keychain 账户绑定到同一 profile。启动时核对已保存的加密身份凭据，数据库或密钥错配会阻止 Host 启动。筹备中的 profile 保留原 UUID，首次启动被中断后可用原账户继续。锁驻留在数据目录，直到 helper 排空请求。macOS 26 ARM64 上两个独立启动的临时签名 helper 用同一 Keychain 读回加密 SQLite 条目。隔离测试还拒绝并发路径别名、丢失和错误密钥、缺失或符号链接的身份文件、以及替换的数据库。独立的签名 Swift 端到端步骤再次通过真实系统剪贴板、授权、撤销和关闭验收。Linux 通过 88 项 Host 与 183 项 Platform 测试，各有 1 项按既有条件忽略。Windows 通过 Host、Platform 测试目标与样件的编译检查。发行签名和最终 launcher 的 Keychain ACL 仍需验收。 [Darwin 持久化与 Swift 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46121528525)

- **C2 CI 模式与结果核对**。集中解析命名模式及迁移期旧开关，昂贵任务前拒绝互斥请求。Windows Surface 性能模式直接使用本次比较产物完成原生功能前置，Service 启动堆栈与 Surface 崩溃各有独立模式。汇总核对必需 job、矩阵实例和关键步骤，源码准入绑定当前 attempt，Release 兼容明确跳过的诊断任务。手动源码准入首次用时 15 分 31 秒，双 x64 目标和源码凭据均已核对。以该次 GitHub 任务记录复核扩展后的 32 项必需结果。Darwin 命名模式通过；互斥请求只执行计划和汇总；旧 Darwin 开关的受控取消不会留下准入凭据。保留的全量模式尚未在这组改动上实跑，tag 的缓存保存调整也尚无发版计时。
  [源码准入](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46122651572) [Darwin 专项](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46122917991) [非法组合](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46122651145) [取消检查](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46123190428)

- **C3 合并重复构建和测试**。在并行的 Frontend job 执行一次可移植工具契约。各平台仍使用实际二进制运行 BuildInfo、独立归档和 SQLite 验收，Linux 保留私有挂载与凭据隔离负控。一次 Cargo 调用生成 CLI 和 Service，随后验证 Tauri 构建前后的两个文件哈希。Windows 将 5 项真实剪贴板测试合为一次串行调用，并复用已构建的 Go 测试程序验证中键钩子。原源码准入首次运行 15 分 31 秒，修正后的 `8a4c2e89` 首次运行 12 分 29 秒。Windows 剪贴板步骤从 40 秒降到 13 秒。整体差值也包含缓存影响，两次 Windows Rust cache 保存步骤分别为 106 秒和 0 秒。已下载后一次准入凭据，核对 SHA、run/attempt、双 x64 目标及 GitHub 记录中的 42 项必需结果。tag 的首轮发布耗时仍待实测。 [源码准入实跑](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46124948797)
- **C3 当前 Windows MyGo 安装包**。`d7d64c59` 的专项在托管 Windows x64 运行 4 项真实 NSIS 钩子用例，随后验证当前安装器的恢复、升级、文件锁、卸载和重装。产物以 artifact ID `15255544468` 留在该次测试运行。 [安装包专项](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46125125204)
- **C4 直接验证真实实现**。Windows 点击规划的 11 个用例直接导入运行时模块，另保留 75 项注入 OS 的测试。PowerShell 开发检查使用实际进程环境验证缺失、空值、文本和值恢复失败。当前 NSIS 的原 GUI 与升级测试仍由六场景验收。退役无人调用的 `test:e2e` 和 `@playwright/test`，保留 Playwright 的生产 WebView 冒烟。本地生产构建和实际 Chromium 页面设置、字符目录搜索通过。 [PowerShell 托管验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46128671130)
- **C2/C4 全量矩阵复核**。`d7d64c59` 的四平台桌面、四份 standalone、两架构 Flatpak、六项 NSIS 和开发任务实际通过。历史 alpha.9 验收把已发布的 WebView 窗口当成当前 Native 窗口，因此原运行和汇总失败。`a8675bf9` 的 Windows 托管复验执行了普通及非 ANSI 路径的真实安装、粘贴、升级、文件锁、卸载与重装，两个路径都通过。 [首轮全量矩阵](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46127684498) [历史包复验](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46128671130)
- **R3 GPU 启动读探针**。Windows 和 Linux x64 各运行 3 项 Tauri 启动探针测试。新增父进程持有无 sidecar 的 SQLite 读取事务、子进程执行真实探针的检查，父测试核对子用例确实执行。释放锁后同一个数据库恢复正常读值。原有坏库、回滚事务、活跃 WAL、已 checkpoint WAL 和文件字节完整性检查均通过。

### Darwin 首发拓扑

首发按直接分发的 macOS ARM64 `.app` 设计，沿用 Swift package 声明的 macOS 13 编译目标。Swift 启动器从 `Contents/Helpers` 启动同一包内的 Rust Host，继承管道传递短期授权并绑定 helper 生命周期。私有 Unix socket 只传现有帧协议。Host 继续持有 Core、profile 和采集所有权。应用包由内向外签名，启动器核对应用包与内嵌代码后再启动，并在连接上核对实际 helper 的签名身份。暂按用户会话内运行、Hardened Runtime 和 Developer ID 直接分发准备，发行阶段再核对公证和对用户 Keychain 的访问。 [Apple 嵌套代码签名](https://developer.apple.com/library/archive/technotes/tn2206/_index.html#//apple_ref/doc/uid/DTS40007919-CH1-TNTAG201)

`5d5fdd14` 的 macOS 26 ARM64 托管专项实际构建并临时签名测试 `.app`，通过裸启动器拒绝、独立进程持久 profile，以及经签名包发现并启动真实 Host 的 Swift 会话。该会话验证对端身份、授权、历史、快照、事件、系统剪贴板和退出。macOS 13 的运行验证、Developer ID 签名与升级后的 Keychain ACL 仍需取得独立证据。 [签名应用包验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46128925341)
