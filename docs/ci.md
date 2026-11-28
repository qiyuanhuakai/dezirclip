# CI 验证范围与发布基线

本页沿用 DZC-ARCH-001 的 C0 验收结构，记录以 `ba0ad71f7b9d8caddac45d0bc5588254a8273482`（`v0.2.0`）为基线的当前工作流。后续修改 workflow 时应同时更新本页。

**正常 tag 发布调用候选源码验收，不构建或发布 Flatpak。** 手动 full、日常开发检查和 tag 发布是不同路径。以下“执行”描述配置选中的步骤。

## 触发与任务范围

入口见 [CI 工作流](../.github/workflows/ci.yml) 和 [Release 工作流](../.github/workflows/release.yml)。实例数按矩阵展开后的实际 job 计算，不计被跳过的模板或复用工作流的外层名称。

| 入口 | 当前要求的任务 | 覆盖限制 |
|---|---|---|
| `master` push / 任意 PR | 前端、Rust 与 Go 开发检查，特定路径变更时增加历史 MyGo 安装验收 | 开发检查随正常 PR 和 push 自动执行 |
| 手动 `mode=development` | 开发检查及历史 MyGo 安装验收 | 手动触发缺少可靠 diff 基线，历史安装器按需执行规则要求运行 |
| 手动 `mode=full`（未指定专项时的默认模式） | 开发检查、历史 MyGo 安装验收、四目标桌面和当前 Windows x64 MyGo 安装验收、四目标 standalone、六场景 NSIS、两个 Flatpak | Windows ARM64 的桌面和 standalone 任务是交叉构建 |
| `mode=source-acceptance` 或 Release 复用调用 | 前端、Rust 与 Go 开发检查，Linux / Windows x64 的源码、真实客户端和桌面验收，写出绑定候选提交和 attempt 的凭据 | 正常 tag 的独立打包和最终包验收按下文执行 |
| `v*` tag push | draft、四目标 standalone、四目标双桌面构建与验收、候选封存、公开与通道同步 | 以本页“正常 tag 发布”部分的实际门槛为准 |

桌面专项同时运行轻量的前端工具契约步骤。生成检查、前端安装、单测和构建仍随开发、全量和源码准入模式执行。

日常的历史 MyGo 安装验收使用 **alpha.9 已发布 payload + 当前 checkout 的安装器源码**，实际检查该版本的 WebView 主窗口、原生面板、粘贴和安装周期。范围比较 CI/Release workflow、MyGo 安装器、打包、测试夹具、进程凭据和图标等路径。缺少可靠基线时也执行。实际来源见 [历史 payload 准备器](../scripts/package-published-mygo.py)。

| 专项模式 | 当前执行范围 |
|---|---|
| `darwin-native` | macOS ARM64 文件、IPC、签名对端、持久化 profile、Swift 会话与最终签名包的部署目标 |
| `native-file-drag` | Linux x64 和 Windows x64 原生文件拖放、目标互动及现有截图验收 |
| `historical-windows-memory` | 固定 alpha.8 / alpha.9 的 Windows 内存比较 |
| `mygo-package` | Windows x64 当前 MyGo 候选安装、运行验收和安装包归档 |
| `linux-release-inputs` | Linux x64 桌面检查后生成绑定源码和产物身份的构建输入 |
| `windows-performance` | 构建 Windows 比较用产物，对同一份产物执行功能验收和 ABBA 采样 |
| `windows-crash` | 选择 `windows_crash_target=service-startup` 或 `surface`。只有 `surface` 可选 `windows_crash_build_profile=debug` |

每次手动触发只接受一个模式。脚本迁移期间，`mode=auto` 继续接受一个旧专项开关。旧 `full_validation` 只在未选择专项时决定默认范围，`false` 选择 development。旧输入供已经使用 `gh workflow run` 的专项调用迁移，仓库内的 Release 暂时继续传 `source_acceptance=true`。调用迁至命名模式并核实旧接口已无消费者后移除对应参数。命名模式和旧专项开关同时出现时，前置任务会拒绝请求。

结果汇总通过 GitHub Actions 的任务记录核对固定清单。每个必需矩阵实例和所列的关键步骤都要实际成功，缺失、跳过、取消和失败分别显示。普通 CI 的部分重跑沿用 GitHub 认定的同一 run 最新结果。候选源码验收核对当前 attempt，核对通过后才生成既有格式的凭据。

tag 源码准入的 Tauri 任务会恢复可用的 Rust 缓存，跳过本 tag 独占缓存的保存步骤。手动源码准入实跑中，Windows 桌面任务最后的 Rust 缓存保存耗时 1 分 46 秒。跨 tag 的缓存访问范围见 [GitHub 缓存说明](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)。 [手动源码准入](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46122651572)

PR 更新和新的 master push 会取消同组旧 CI。手动 full 不适用该取消条件。Darwin 专项使用独立的并发组，可与同一分支的 Windows/Linux 检查同时运行。普通日常检查的耗时目标仍为 10 分钟以内；排队、外部下载和真实原生验收耗时须分别记录，超时或取消不能算成功。

Darwin 专项以 macOS 13.0 为 Swift 和 Rust 的编译目标。签名后核对包的 `LSMinimumSystemVersion`、启动器与内嵌 helper 的 ARM64 架构，以及两份 Mach-O 的 macOS 最低版本。实际运行使用 GitHub 托管 macOS 26 ARM64。macOS 13 实跑需要独立环境。 [GitHub 托管镜像范围](https://github.com/actions/runner-images) [Rust Darwin 部署目标](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html)

### 源码工具与平台验收分工

| 条件 | 主执行位置 | 执行边界 |
|---|---|---|
| 可移植的打包、发布恢复、Flatpak 验证器和诊断脚本契约 | 前端任务的共享工具检查 | 同一轮运行一次。Standalone 保留归档 ABI、解包与实际运行验收。Flatpak 保留实际包的安装和运行验证 |
| 真实 SQLite 夹具、Rust 构建身份和平台打包工具 | Linux、Windows 可执行的桌面任务 | 保留对应 Python、SQLite、Rust 工具。Linux 的私有挂载和凭据负例须实际通过 |
| Windows NSIS 卸载钩子夹具 | 当前 MyGo 包的打包验收 | 安装 NSIS 后必跑，零用例或跳过视为失败 |
| Windows MyGo Native 测试安装包 | Windows x64 桌面任务 | 复用本轮原生验收的 Go 可执行文件，暂存后核对实际 BuildInfo 和构建证明的 SHA-256。Rust CLI、Service 仍按 release profile 构建 |
| Windows WebView 环境恢复 | Windows 开发任务和六场景 NSIS | 开发任务直接调用共享 PowerShell helper，使用真实进程环境测试缺失、空值、文本和失败。完整 NSIS 验收继续检查原 GUI 生命周期和升级 |
| Standalone Companion 归属 | Tauri 桌面任务 | CLI 与 Service 一次构建。Tauri 测试和实际 Desktop 链接前后核对两个可执行文件的 SHA-256 |
| Tauri Web 前端与原生构建 | Frontend、Windows 原生 Surface 验收和 Linux release-inputs 专项 | Frontend 保留生产 Web 构建与浏览器检查。桌面矩阵仍安装 Surface token 校验所需依赖，仅在嵌入 Web 产物的场景额外构建前端。Windows Surface 验收复用 `--all-targets` 随备份集成测试产出的 `custom-protocol` 可执行文件，核对 Cargo 产物记录和 SHA-256 |
| Windows 原生剪贴板和阻塞 UI 的中键钩子 | Windows 桌面任务 | 真实剪贴板用例在同一测试进程串行执行。中键钩子复用 MyGo 库验收使用的 Go 测试程序，并确认用例实际通过 |
| Linux 原生剪贴板、输入和快捷键 | Linux Rust 开发任务 | 复用当轮 Cargo JSON 中的 Platform 单测程序，在私有 Xvfb/DBus 中执行。精确核对 15 项实际结果，包含 XFixes 服务器重启和无合成器的 Wayland 建连取消 |

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

`release.yml` 并行创建 draft、构建产物与执行候选源码验收。standalone 上传等待其构建和 draft，按当前 attempt 的固定 artifact ID 下载四份归档，核对校验文件、绑定 draft ID 并读回已上传 asset。桌面上传任务按源码 SHA 和 attempt 下载四个生产 artifact ID 对应的文件，先验证 12 个签名和 20 份桌面归档，再绑定 draft ID 上传。归档检查核对安装路径、可执行文件架构和适用的包元数据，并确认 Arch 二进制和 AUR 配方对应候选中的 Debian 包。Windows x64 的 Tauri 和 MyGo 安装包各交给独立 runner。验证从固定签名旧版 v0.2.0 升级、三个安装文件的 SHA-256、启动、重装、卸载和数据保留。独立封存任务按固定 artifact ID 下载源码记录、上传读回以及 Windows 和 Linux 的原生验收。封存附件绑定原 draft ID、每项资产的摘要、原 producer 及预期更新 manifest。正常 tag 的公开任务需要封存 job 成功，并独立核对原封存资产身份、签名与写入前后的远端状态。 [发布脚本](../scripts/updater-release.mjs)

每个目标用一次 Cargo 构建产出 Tauri、桌面配套 CLI 和 Service，随后 Tauri 打包阶段复用已暂存的二进制，MyGo 使用相同目录。独立 CLI 下载保持原 ABI 构建边界。生产 Web bundle 的浏览器测试在源码验收执行一次。本地以同一路径构建并解包 Windows x64 NSIS 和 Linux x64 deb，两个 helper 的 SHA-256 和实际 CLI/MCP 集成通过。

历史基线 `v0.2.0` 的 tag push 首次运行，从触发到公开和更新通道同步任务完成用时 18 分 09 秒。当前架构候选的首次发布总耗时仍需实测。 [v0.2.0 发布运行](https://github.com/qiyuanhuakai/dezirclip/actions/runs/45952455038)

当前门槛保留仓库/tag/SHA/run/attempt、必需 job、资产目录及时间、签名/版本/文件名、updater 下载读回和防降级检查。恢复含 Arch 资产的候选时也核对原 Release run 的源码验收工作。只读的 `replay-windows-candidate` 已用公开签名安装包验证两个桌面从 v0.2.0-rc5 升到 v0.2.0。未创建新 release，也未切换更新通道。Linux x64 和 ARM64 已对 v0.2.0 的 AppImage、DEB 和 RPM 执行隔离启动验收。Linux 最终包检查并行启动 Ubuntu 和 Fedora 的隔离容器，用 `dpkg`、`rpm` 验证两个桌面的安装、从固定上一版本升级、重装、卸载及 profile 保留。候选封存核对这四组包管理器结果及安装后程序的哈希。只读重放使用 v0.2.0-rc5 和 v0.2.0，已在 Linux x64 和 ARM64 各通过四组原生事务和六份最终包的 X11 运行。正常 tag 使用 v0.2.0 作为基线。Linux x64 tag 验收另用无网络 Arch 容器对两份原始候选包执行 pacman 安装、升级、重装和卸载，封存核对原始归档摘要与三个已安装程序的摘要。已发布的 v0.2.0 没有 Arch 包，升级起点沿用验证过签名的 Debian 字节和同一生产配方，三个旧程序与 dpkg 实装结果逐一比对。只读重放也从已验证的 Debian 包重建当前侧的 Arch 包。真实 40+1 tag 链路和首次总耗时仍待实测。 [双桌面 Windows 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46040562056) [双架构 Linux 验收](https://github.com/qiyuanhuakai/dezirclip/actions/runs/46133365670)

### 当前发布资产集合

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

含 Arch 打包步骤的运行必须通过该步骤并上传四项 Arch 产物。旧 dual 运行（包括已发布的 v0.2.0）使用原有 36 项合同。当前发布要求 40 项产品资产和唯一的 `dezirclip-candidate.json`，拒绝其它附件和 Flatpak。两种 Windows 桌面均只分发安装器。产品和封存附件分别受上传 job 和封存 job 的时间约束。

手动恢复沿用 Release 的两个双桌面入口。封存已成功时读取原凭据。如果原封存被取消但附件已上传，`Review interrupted release seal` 从原 attempt 下载七份成功报告，重建并读回原附件。恢复发布器检查新复核的 job、artifact ID、摘要、时间及原附件身份，并将两次运行分别记入同步回执。复核使用 GitHub 列出 draft 所需的仓库权限，脚本仅允许调用读取 Release 证据的 GET 请求。 [GitHub Releases API](https://docs.github.com/en/rest/releases/releases#list-releases)

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
| 完整 Wayland 合成器矩阵仍为手动验收 | Linux 自动任务覆盖 X11、Wayland 快捷键回退负控及建连取消。实际 Wayland 合成器和 CLI MIME 测试由 [Wayland 实验支持](./wayland-experimental.md) 的隔离脚本运行，按协商协议分别留证 |
| 当前候选与发布闭环仍待实测 | 新 tag 完整发布链、首次总耗时，以及实际取消封存后的托管恢复均待实测。Linux DEB/RPM/Arch 系统级安装和升级已通过只读重放 |

源码准入、共享 NSIS 样件和 Windows x64 最终包验收已接入。当前含 Arch 的产品目录包含 40 项。Windows ARM64 产物按现有交叉构建级别验收。

## 名称消费者、产物命名与仓库外设置

- 发布 verifier 以 job 显示名称作精确匹配：当前 `dualReleaseJobs`、以及历史 `fullSourceJobs`、`nativeReleaseJobs`、`releaseGateJobs`、`optionalReleaseJobs` 均在 `updater-release.mjs`。改名、复用前缀、skip 规则或新增后置 job 时要同步该消费者及 [updater-release.node.mjs](../scripts/tests/updater-release.node.mjs)、[packaging-sidecars.node.mjs](../scripts/tests/packaging-sidecars.node.mjs) 等连接契约，不能把所有 skipped 一概视为通过。
- `ci.yml` 的复用验收 artifact 使用 `acceptance-cli-*`、`acceptance-flatpak-*`；正常 Release 的 standalone 使用 `release-cli-<attempt>-<target>`，通过固定 ID 下载。当前正常 tag 没有 `release-flatpak-*` 生产者。
- Release 菜单已退役 alpha.2 草稿恢复和 alpha.9 MyGo 安装器补传。两项资产已经公开，草稿恢复和同名补传各自会拒绝该状态。当前双桌面恢复仍绑定原 tag、run ID 和 attempt。两个历史 reusable workflow 及其回归脚本保留，供公共 helper 迁移和离线复核。 [alpha.2](https://github.com/qiyuanhuakai/dezirclip/releases/tag/v0.2.0-alpha.2) [alpha.9](https://github.com/qiyuanhuakai/dezirclip/releases/tag/v0.2.0-alpha.9)
- 仓库 API 核对时，`master` 没有经典分支保护或适用规则集。统一的计划与实际执行汇总由 [CI policy](../scripts/ci_policy.py) 实施。外部 required checks、组织 token 策略、environment 审批和 runner 配额仍需按实际配置核对。

## 耗时与验证记录

此前文档记录的 [运行 42557275483](https://github.com/qiyuanhuakai/dezirclip/actions/runs/42557275483) 中，Windows Tauri 约 19.4 分钟、Flatpak x64 约 14.8 分钟，但整次运行失败；[PR 运行 42560409914](https://github.com/qiyuanhuakai/dezirclip/actions/runs/42560409914) 记录为 2026-11-09 04:11:27–04:16:45 UTC，约 5 分 18 秒且全部成功。这些保留为历史记录，本次未重新查询其 API，也不是当前基线完整 CI/Release 的成功耗时证明。

当前基线可对照 [v0.2.0 正式发布](https://github.com/qiyuanhuakai/dezirclip/actions/runs/45952455038) 和 [Arch、X11 修复分支开发 CI](https://github.com/qiyuanhuakai/dezirclip/actions/runs/45984601736)。正式版首次运行从启动到 Stable 生效为 18 分 01 秒。修复分支的手动基础 CI 通过，涵盖 7 个基础实例和历史 MyGo 安装器。GitHub 的 master 分支信息报告未启用经典分支保护。规则集、组织 token 策略、配额与完整候选验收仍需各自核对。
