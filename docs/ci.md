# CI 验证范围与发布基线

本页沿用 DZC-ARCH-001 的 C0 验收结构，记录以 `ba0ad71f7b9d8caddac45d0bc5588254a8273482`（`v0.2.0`）为基线的当前工作流。后续修改 workflow 时应同时更新本页。

**正常 tag 发布调用候选源码验收，不构建或发布 Flatpak。** 手动 full、日常开发检查和 tag 发布是不同路径。以下“执行”描述配置选中的步骤。

## 触发与任务范围

入口见 [CI 工作流](../.github/workflows/ci.yml) 和 [Release 工作流](../.github/workflows/release.yml)。实例数按矩阵展开后的实际 job 计算，不计被跳过的模板或复用工作流的外层名称。

| 触发 | 当前选择的任务 | 覆盖限制 |
|---|---|---|
| `master` push / 任意 PR | `frontend` 1、`rust-development` 4、`go-development` 2，共 7 个基础实例；特定路径变更再加 `mygo-installer` | 没有 full 的四目标桌面、standalone、六场景 NSIS 和 Flatpak |
| 手动 CI，`full_validation=false`，其它开关关闭 | 上述 7 个基础实例，加历史 `mygo-installer`，共 8 个 | 手动触发没有 diff 基线，历史 installer 的 scope 默认要求执行 |
| 手动 CI，默认 `full_validation=true`，其它开关关闭 | 基础 7 + 历史 installer 1 + 矩阵选择 1 + Tauri 4 + standalone 4 + NSIS 构建 1 与验收 6 + Flatpak 2，共 26 个 | Windows ARM64 是交叉构建；历史/合成 installer 与当前发布包各有验收入口 |
| Darwin 基础 `macos_native_probe=true` | 手动在 GitHub 的 macOS ARM64 runner 验证原生文件、IPC 和签名对端，命令同时设置 `full_validation=false` | 不计入 Windows/Linux 发版门槛 |
| 复用 `ci.yml` | `workflow_call` 的 `full_validation` 默认 true。Release 显式调用候选源码模式 | 不运行完整的打包测试矩阵 |
| 候选源码验收 `source_acceptance=true` | 前端、Rust 与 Go 开发测试，Linux/Windows x64 真实客户端和桌面验收，汇总绑定被测 SHA | 可单独运行，与全量安装和升级验收互补 |
| `v*` tag push | draft、四目标 standalone、四目标双桌面构建/上传、`publish-dual` | 以本页“正常 tag 发布”部分的实际门槛为准 |

日常 `mygo-installer` 检查的是**固定 alpha.9 已发布 payload + 当前 checkout 的安装器源码**。scope 比较 CI/Release workflow、MyGo installer/packaging/harness、进程 receipt 和图标等路径；没有有效 diff 基线或比较失败时也要求执行。它不是当前 beta.4 payload 的安装验收。实际来源见 [历史 payload 准备器](../scripts/package-published-mygo.py)。

专项输入可以叠加。候选源码验收使用自己的两个 x64 目标，遇到同时请求专项的配置会明确失败。

| 输入 | 当前调度事实 |
|---|---|
| `native_file_drag_only`、`windows_release_memory` | 分别增加原生文件拖放/截图检查、已发布 alpha.8/alpha.9 内存比较；不会自动关闭基础任务或默认 full |
| `windows_x64_mygo_test_package`、`windows_service_crash_stacks`、`linux_release_inputs` | full=false 时各自只选 Windows x64 或 Linux x64；同时请求时取目标并集 |
| `windows_surface_performance` | full=false 时也选择 Windows x64 前置验收，frontend 和所选桌面任务成功后执行性能比较 |
| `windows_surface_crash_diagnostics` | 有独立的手动诊断条件；不等同于性能比较或完整原生验收 |

PR 更新和新的 master push 会取消同组旧 CI。手动 full 不适用该取消条件。普通日常检查的耗时目标仍为 10 分钟以内；排队、外部下载和真实原生验收耗时须分别记录，超时或取消不能算成功。

## 测试入口与证据种类

主入口在 `ci.yml`；下表区分源码契约、真实客户端、原生行为和最终包。脚本名字含 `native` 或 `performance` 本身不证明执行了原生操作或性能测量。

| 类别 | 当前主入口与对象 | 结果含义 |
|---|---|---|
| 前端、生成一致性和工具纯逻辑 | `frontend`：i18n、版本、Surface tokens、协议生成、CSS、Node/Python fixture 与发布逻辑检查；`pnpm run test:unit`、前端 build | Vitest 发现 `src` 下的测试；fixture/mock 通过不等于真实窗口或发布通过 |
| Rust 业务与客户端 | `rust-development`：Linux/Windows 各跑 `core-tests` 和 `desktop-check` | 前者使用 workspace/all-targets 和 CLI/Service 的 `native-screen` feature，后者执行桌面 `cargo check` |
| Go 客户端和 UI 逻辑 | `go-development`：Linux/Windows 分别在 `clients/go` 和 `frontends/dzc-surface` 跑 `go test ./...` | Deziro UI Native 和 presets 使用独立仓库的固定版本。PNG 是渲染诊断 |
| 真实 CLI / TUI | 日常 Windows `core-tests` 先构建本次 CLI，再运行 ConPTY 用例。full 的 standalone integrations 使用隔离 ABI 产物 | ConPTY 零用例或跳过会失败；Windows ARM64 按 static-only 验证 |
| 真实 Host 客户端 | `tauri` 可执行目标在本次 CLI/Service 构建后运行 `test-local-host.py`、`test-local-clients.py` 和 CLI BuildInfo 契约 | 单独保存 CLI/MCP 报告，覆盖授权、撤销、显式持久化确认和 Host 停止后拒绝回退 |
| 桌面与原生 OS | full/对应专项的 `tauri`：Linux x64/ARM64、Windows x64 的真实 harness；Windows clipboard、Explorer、private lease 等显式用例 | Linux MyGo 使用隔离 X11；Windows ARM64 `can_test=false`，不能称为四目标均已原生执行 |
| 安装与升级 | `native-windows-upgrade` 六场景；历史 `mygo-installer`；当前 MyGo installer 的专项步骤 | 来源与构建形态见下文，不能互换成正式候选证据 |
| standalone 最终 archive | [standalone-cli.yml](../.github/workflows/standalone-cli.yml)：隔离构建、依赖/架构检查、解包及适用平台运行 | Linux 还在 AlmaLinux 8、Ubuntu 22.04、Debian 12 运行；Windows ARM64 只做 static-only |
| Flatpak 最终环境 | [flatpak.yml](../.github/workflows/flatpak.yml)：Linux x64/ARM64，构建并经 `verify-flatpak.py` 验证后导出 | 在 org.gnome.Platform runtime 验证 OCR/paste；是 full 的验收 artifact，当前 tag 不发布 |
| 性能与诊断 | `scripts/performance/tests` / frontend 的比较器与 fixture 单测；手动性能或崩溃入口 | 工具单测、崩溃诊断与真实性能测量分别报告，不互相代替 |

Linux/Windows x64 的 Go 单测归开发矩阵，桌面验收保留真实 Go/Rust 客户端和原生桌面。Tauri 测试任务只运行其 Cargo 包的套件。Windows NSIS 的纯打包检查集中到 fixture 构建任务，完整验证的跨目标 Python discovery 仍待收口。

### 六个 NSIS 场景的实际来源

入口是 [test-native-windows-upgrade.ps1](../scripts/test-native-windows-upgrade.ps1)。同一任务用当前源码各构建一次 alpha.1 和 alpha.2，保留 debug、无 updater 签名和 `createUpdaterArtifacts=false`。各场景使用 producer 输出的 artifact ID 下载产物，核对安装器、NSIS 输入及源码 SHA，再在独立 Windows runner 运行。共享样件与固定历史安装器的六个场景已在 [GitHub Actions](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46011542518) 通过。源代码会由版本设置器临时修改，因此 fixture 的来源标为 `modified_checkout`。 [构建脚本](../scripts/build-native-windows-fixture.ps1)

| 场景 | 当前输入与目的 |
|---|---|
| `LegacyUninstall` | 固定已发布 `0.1.20`；复现旧卸载器破坏设置/Explorer 的行为，作为历史负向基线 |
| `OldToNew` | 已发布 `0.1.20` → 当前源码生成的 alpha.2 fixture；检查迁移、GUI、卸载和设置保留 |
| `PublishedAlphaToAlpha` | 固定已发布 alpha.1 → alpha.2 fixture；核对旧版本身份/数据库 schema 后升级 |
| `OwnedUpgrade` | 同源码 alpha.1 fixture → alpha.2 fixture；检查系统设置所有权 receipt 跨升级及卸载恢复 |
| `OwnedExternalEdit` | alpha.2 fixture；取得设置所有权后模拟外部修改，卸载须保留较新的外部值 |
| `DeniedGuiUpgrade` | alpha.2 fixture；检查 GUI 升级被拒绝时的处理和 Explorer 保留 |

这些场景保护历史迁移和安装器行为，不能证明“上一受支持正式版本 → 当前已签名最终包”已经通过。当前四目标桌面 job 内的 MyGo 安装器步骤只在 `windows_x64_mygo_test_package=true` 的手动触发中可达：它的 push 分支被外围 `tauri` 条件挡住，默认 full 也不选择这个步骤。

## 正常 tag 发布

`release.yml` 并行创建 draft、构建产物与执行候选源码验收。standalone 上传等待其构建和 draft，按当前 attempt 的固定 artifact ID 下载四份归档，核对校验文件、绑定 draft ID 并读回已上传 asset。桌面上传任务按源码 SHA 和 attempt 下载四个生产 artifact ID 对应的文件，先验证 12 个签名和 20 份桌面归档，再绑定 draft ID 上传。归档检查核对安装路径、可执行文件架构和适用的包元数据，并确认 Arch 二进制和 AUR 配方对应候选中的 Debian 包。Windows x64 的 Tauri 和 MyGo 安装包各交给独立 runner。验证从固定签名旧版 v0.2.0 升级、三个安装文件的 SHA-256、启动、重装、卸载和数据保留。首次发布任务按固定 artifact ID 下载源码记录、包检查、Windows 原生验收和上传读回，将结果与原 draft 资产的 ID、大小和 SHA-256 对应后再公开 release 并同步 stable/preview。 [发布脚本](../scripts/updater-release.mjs)

每个目标用一次 Cargo 构建产出 Tauri、桌面配套 CLI 和 Service，随后 Tauri 打包阶段复用已暂存的二进制，MyGo 使用相同目录。独立 CLI 下载保持原 ABI 构建边界。生产 Web bundle 的浏览器测试在源码验收执行一次。本地以同一路径构建并解包 Windows x64 NSIS 和 Linux x64 deb，两个 helper 的 SHA-256 和实际 CLI/MCP 集成通过。Release 首次运行计时仍需候选实测。

当前门槛保留仓库/tag/SHA/run/attempt、必需 job、资产目录及时间、签名/版本/文件名、updater 下载读回和防降级检查。恢复含 Arch 资产的候选时也核对原 Release run 的源码验收工作。只读的 `replay-windows-candidate` 已用公开签名安装包验证两个桌面从 v0.2.0-rc5 升到 v0.2.0。未创建新 release，也未切换更新通道。Linux 独立最终包运行、候选封存和新候选的完整计时仍待执行。 [双桌面 Windows 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46040562056)

### 当前发布分支的精确资产集合：40 项

以 `updater-release.mjs` 的 `verifiedReleaseAssets(..., dual=true)` 为准，版本与文件名按现有规则生成；下表是格式数量说明，不是第二套文件名配置。

| 产品资产 | 架构/格式 | 文件数（含签名或 checksum） |
|---|---|---:|
| Tauri updater | Windows x64/ARM64 NSIS、Linux x64/ARM64 AppImage，各带 `.sig` | 8 |
| Tauri deb | amd64/arm64，各带 `.sig` | 4 |
| Tauri rpm | x86_64/aarch64 | 2 |
| MyGo updater | Windows x64/ARM64 NSIS、Linux x64/ARM64 AppImage，各带 `.sig` | 8 |
| MyGo deb | amd64/arm64，各带 `.sig` | 4 |
| MyGo rpm | x86_64/aarch64 | 2 |
| standalone CLI | 四目标 archive，各带 `.sha256` | 8 |
| Arch Linux x86_64 | Tauri 和 MyGo 各有一个 pacman 包和一个 AUR 配方归档 | 4 |
| 合计 | 28 个原桌面文件 + 8 个 standalone 文件 + 4 个 Arch 文件 | **40** |

含 Arch 打包步骤的运行必须通过该步骤并上传四项 Arch 产物。旧 dual 运行（包括已发布的 v0.2.0）使用原有 36 项合同。当前集合没有 Flatpak，也没有 `dezirclip-candidate.json`，额外附件会被严格目录检查拒绝。后续 C5 在启用 Arch 发布的候选上以 40 项产品资产和一项封存证据为基础。

### 当前发布验证级别

| 对象 | 当前正常 tag 路径的证据 | 尚不能据此声称 |
|---|---|---|
| standalone Linux x64/ARM64 | archive 解包、真实 CLI/历史操作，以及三个 Linux runtime 执行 | 全部桌面/GUI 行为已验证 |
| standalone Windows x64 | 解包后的 CLI 真实运行与验包 | 完整源码和 integrations 已运行；`package-only=true` 会跳过它们 |
| standalone Windows ARM64 | 架构、依赖与静态 CRT 等 static-only 检查 | 在 ARM64 Windows 上运行过 |
| MyGo Linux | deb payload hash、deb/AppImage BuildInfo 检查 | 最终包 GUI/安装/升级或 RPM 安装均已通过 |
| Tauri 和 MyGo Windows x64 | 独立 runner 从固定上一发布版升级，核对实际安装文件、启动、重装、卸载和历史记录。MyGo 另外保留同包应用内更新 smoke | 新候选中的锁文件和中断恢复各场景已经通过 |
| MyGo Windows ARM64 | 交叉编译及 proof/static 检查 | 实际安装、GUI 或输入已验证 |
| 两桌面的 updater 资产 | 四目标 updater 文件的签名、版本/文件名及下载读回 | 每个非 updater 出货资产已有同等内容与构建 hash 绑定 |

`package-only=true` 仍会构建、验包并运行适用的 CLI/runtime；不应描述为“只打包完全不测”。Windows ARM64 当前没有对应原生 runner 验收；静态 CRT 契约属于 standalone CLI，桌面/helper 按各自既有依赖契约检查。

发布 desktop job 将 release profile 覆盖为 thin LTO、codegen-units=8；根 Cargo 默认是 full LTO、codegen-units=1。比较性能、缓存或产物身份时须记录生效配置，同 SHA 不等于同字节。

## 已知未执行缺口与后续边界

以下是当前配置的限制，尚未通过本次文档修订解决：

| 缺口 | 定位与解释 |
|---|---|
| Schema 24 验收尚未接入真实 CLI | [test_sqlite_history_fixture.py](../scripts/tests/test_sqlite_history_fixture.py) 的跨连接用例和 seeder 需要 `DZC_TEST_BIN`，部分 seeder 还依赖 Pillow/psutil。BuildInfo 已安排在本次 CLI 构建后单独运行 |
| 原生测试的“未执行”可能显示通过 | [capture/subscription.rs](../crates/dzc-platform/src/capture/subscription.rs)、[shortcut/x11.rs](../crates/dzc-platform/src/shortcut/x11.rs) 部分测试缺 opt-in 时直接 return；普通 Rust test 成功不能计为这些原生行为成功 |
| Wayland 自动覆盖未接入 | 当前 Linux 产品 harness 是 X11；[Wayland 实验支持](./wayland-experimental.md) 的手工脚本不能算作本次 CI 已执行 |
| 当前候选与发布闭环仍有缺口 | Linux 格式的独立安装与运行、不可变候选附件，以及取消或频道同步中断后的恢复尚待完成 |

源码准入、共享 NSIS 样件和 Windows x64 最终包验收已接入。当前含 Arch 的产品目录包含 40 项。Windows ARM64 产物按现有交叉构建级别验收。

## 名称消费者、产物命名与仓库外设置

- 发布 verifier 以 job 显示名称作精确匹配：当前 `dualReleaseJobs`、以及历史 `fullSourceJobs`、`nativeReleaseJobs`、`releaseGateJobs`、`optionalReleaseJobs` 均在 `updater-release.mjs`。改名、复用前缀、skip 规则或新增后置 job 时要同步该消费者及 [updater-release.node.mjs](../scripts/tests/updater-release.node.mjs)、[packaging-sidecars.node.mjs](../scripts/tests/packaging-sidecars.node.mjs) 等连接契约，不能把所有 skipped 一概视为通过。
- `ci.yml` 的复用验收 artifact 使用 `acceptance-cli-*`、`acceptance-flatpak-*`；正常 Release 的 standalone 使用 `release-cli-<attempt>-<target>`，通过固定 ID 下载。当前正常 tag 没有 `release-flatpak-*` 生产者。
- [release-identities.yml](../.github/workflows/release-identities.yml)、[release-windows-upgrade.yml](../.github/workflows/release-windows-upgrade.yml) 当前仅由 `resume-existing-alpha2` 历史分支调用。beta.4 checkout 不满足该恢复路径的版本/来源约束；菜单存在不代表当前可执行。它们及旧脚本仍有公共 helper/历史行为消费者，不能仅按年代删除。
- branch/tag protection、外部 required checks、environment 审批、组织 token 策略及 runner 配额未在本次本地核对中取得；不能声称已经设置。更改 check 名称前须只读核对实际规则及消费者。当前没有统一“计划与实际执行集合”汇总 gate，绿色界面本身不能证明请求的所有场景均已执行。

## 耗时与验证记录

此前文档记录的 [运行 42557275483](https://github.com/qiyuanhuakai/dezirclip/actions/runs/42557275483) 中，Windows Tauri 约 19.4 分钟、Flatpak x64 约 14.8 分钟，但整次运行失败；[PR 运行 42560409914](https://github.com/qiyuanhuakai/dezirclip/actions/runs/42560409914) 记录为 2026-11-09 04:11:27–04:16:45 UTC，约 5 分 18 秒且全部成功。这些保留为历史记录，本次未重新查询其 API，也不是当前基线完整 CI/Release 的成功耗时证明。

当前基线可对照 [v0.2.0 正式发布](https://github.com/qiyuanhuakai/dezirclip/actions/runs/45952455038) 和 [Arch、X11 修复分支开发 CI](https://github.com/qiyuanhuakai/dezirclip/actions/runs/45984601736)。正式版首次运行从启动到 Stable 生效为 18 分 01 秒。修复分支的手动基础 CI 通过，涵盖 7 个基础实例和历史 MyGo 安装器。GitHub 的 master 分支信息报告未启用经典分支保护。规则集、组织 token 策略、配额与完整候选验收仍需各自核对。
