# Phase 6 MyGo / Tauri 性能对照记录

## alpha.8 → alpha.9 Windows 发布版内存对照（2026-11-09）

本次没有复现日常观察到的约 350 MB → 150 MB。测量使用两个 GitHub Release 的原始 Windows x64 Tauri 安装包，提取程序而不重新编译，在同一台临时 Windows runner 按 alpha.8 / alpha.9 / alpha.9 / alpha.8 顺序执行。alpha.8 没有公开 MyGo 安装包，因此这不是 MyGo 的版本间比较。

每轮新建独立配置和 WebView 数据目录，200 条合成文本、24 张 3840×2160 合成图片，400×600 主窗口、retro 浅色主题、关闭自动 OCR 和贴边隐藏、开启 60 秒空闲销毁。主窗口置顶以避免 CI 焦点变化自动收起。显示及重新唤出后，确认 7 条历史行可见，再稳定 30 秒；每个状态约采样 20 秒、间隔至少 500 ms。隐藏采样始于关闭主窗口 5 秒后，销毁态始于隐藏约 70 秒后。四轮销毁态均仅剩主进程，已跟踪的 WebView2 子进程全部退出；每个其他状态均观察到 7 个应用进程。

以下范围为每版两轮的各轮中位数范围，单位 MiB，指标是 **WebView2 进程工作集之和**：

| 状态 | alpha.8 | alpha.9 |
|---|---:|---:|
| 正常显示 | 315.0–321.6 | 314.7–315.4 |
| 隐藏，尚未销毁 | 247.2–254.3 | 246.2–249.2 |
| 空闲销毁后 | 0 | 0 |
| 重新唤出 | 298.0–299.5 | 297.3–302.4 |

正常显示时，完整应用进程组的工作集为 352.3–358.8 / 355.5–356.1 MiB，私有提交量为 128.6–131.5 / 131.2–132.7 MiB（分别为 alpha.8 / alpha.9）。工作集含共享页，跨进程相加可能重复计入；私有提交量包含非驻留分配，不能与工作集或任务管理器的私有工作集混用。此测量不能确认用户日常读数使用了哪一种口径。

限制：每版仅两轮短时合成负载；没有测量长期滚动、用户自定义主题、大量富文本或用户数据。就绪检查确认行已渲染；初次检查时异步图片完成数为 0–2，稳定阶段之后未再次核对图片完成数。未记录 WebView2 的确切版本号或私有工作集。因此结果支持“此测试条件下未观察到约 57% 的下降”，不能排除其他使用场景的改善，也不能把日常降幅归因于某项代码修改。

[完整四轮测量作业](https://github.com/qiyuanhuakai/dezirclip/actions/runs/42569103460/job/129002575452)成功。`published-windows-memory-1` artifact 保留安装包 SHA-256、每轮 EXE SHA-256、原始进程样本和各状态汇总。该次 workflow 的其他原生适配作业失败，不能将本测量作业成功表述为整个 PR 验收通过。此前探针失败轮未混入本组结果。

## alpha.9 独立桌面的 Linux X11 驻留测量（2026-11-03）

MyGo 快贴和截图窗口由同一个原生进程持有。主窗口使用独立 WebView 进程，悬浮预览在该进程中按需创建原生窗口。关闭主窗口会退出整个 WebView 进程。三个窗口各自通过经过身份验证的本地连接调用同一个 Rust Host。

下表记录 WSL Ubuntu、Xvfb、Openbox、软件渲染下的 release 构建。每次使用新的临时配置、同一条合成记录、相同屏幕尺寸。两边均设置打开主窗口启动，并为 Tauri 开启 5 秒空闲销毁。显示窗口 3 秒后取样，通过窗口管理器关闭主窗口，确认窗口消失，再等 12 秒取样。指标为桌面和服务进程树的 PSS 之和，共享页按比例计入。所有选中进程均可读。

| 实现 | 主窗口打开 / MiB | 关闭后 12 秒 / MiB | 关闭后的进程 |
|---|---:|---:|---|
| Tauri | 430.2–430.6 | 161.8–163.0 | Tauri、WebKit 网络辅助进程 |
| MyGo，同一进程创建 WebView 和原生面板 | 255.0–255.4 | 183.6–184.1 | MyGo、WebKit 网络辅助进程、Rust 服务 |
| MyGo，独立 WebView 进程 | 284.8–285.6 | 125.0–125.1 | MyGo 原生面板、Rust 服务 |

每种 MyGo 布局各跑两次，Tauri 在前后对照中共跑三次。表中范围是这些快照的最小值和最大值。当前 MyGo 主窗口仅提供部分历史操作，打开态的两套界面功能覆盖不同。

另两次真实操作测试依次打开并移出悬浮预览、关闭主窗口、从外部编辑器呼出快贴并粘贴、全屏截图，再拖选 80 × 65 区域截图。收起一秒后，独立进程方案合计为 148.1 和 148.9 MiB。测试还核对了原编辑器焦点、粘贴全文、截图尺寸和冻结像素。X11 验收通过进程描述符检查主窗口及已观察到的 WebKit 子进程均已退出。此操作序列尚无对应的 Tauri 或 Windows 资源数据。

更新日期：2026-11-02。Phase 6 尚未通过，Phase 7 未启动，默认仍使用 Tauri。

Linux X11 新源码 `1fe685c` 的独立完整四轮已完成，并从原件独立复算。MyGo 在此受限合成配置下减少应用进程组合的内存及静态显示 CPU；已预热窗口呼出到原生可见且前台的 p95 从 Tauri 的 62.332 ms 变为 MyGo 的 71.367 ms，增加约 14.5%。此终点是原生窗口代理，首次可交互帧、搜索就绪等仍未测量。新方法不采集线程／上下文切换，观察成本已改变，不能把与旧方法的差异宣称为产品优化。[完整要求](v0.2-mygo-ready.md#p66-性能评估)

Windows `2a2521f` 的新完整四轮对照已通过冻结评估器及独立原件、数学复核：MyGo 更快交付粘贴全文，但原生可见／前台代理的 pooled p95 为 39.606 ms，Tauri 为 27.303 ms；MyGo 收起后的工作集约为 Tauri 的两倍，PrivateCommit 也更高。当前结果不满足更快呼出的候选目标，不据此迁移默认渲染器。旧 MyGo helper 退出 2、Desktop 清理前堆损坏、权限拒绝及映像查询失败的根因仍未关闭；新完整运行与五次未复现不构成根因修复。

## Linux X11 新源码完整对照：process-resource-only-v2

本组使用 `1fe685c424602b2bc1892f1deb0794c2e0b90ad5` 的实际 release pair，Rust 1.99.1、Go 1.27.3。Desktop 为 16,722,680 字节、SHA-256 `e7fe7ba757c4d337e385acb9aa25f545922fecf12e77b8f21c983633bf54bde8`；Surface 为 10,735,879 字节、SHA-256 `8b0e442f1fa1be730a02ef600bae9f343433ee54e14386c814d64c75fe51ab68`。输入来自原 producer [41317927879 / 13088123731](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41317927879/artifacts/13088123731)，原供给及构建身份见后文。

原线程采集 A1 失败后，另行审查、冻结新方法并从头执行 Tauri／MyGo／MyGo／Tauri；没有补点或混入失败轮。新方法保留进程族、创建代际、PSS／USS、CPU 原端点、采集时钟、原生窗口资格、隔离与原对象清理规则，明确不读取线程目录，线程数、上下文切换、稳定线程集及唤醒均为 null／not collected。原生可见且前台的计时代理与资格期限不变。

四个原始 outer 均只执行一次并退出 0。每轮使用新配置，5 次预热、30 次正式激活，显示／收起各 960 次采集；合计 120 次正式激活和 7,680 次资源采集及对应时钟记录。每个状态先稳定 30 秒，以实际前一次开始加 250 ms 为下一到期点，不追赶；首末计数器读窗之间的 CPU 与尾部实际等待分别保留。每侧仅两轮，快照及试次不能视为独立实验重复。

| 指标 | Tauri | MyGo |
|---|---:|---:|
| 已预热原生可见／前台代理 p50 / p95，ms（每侧 60 次） | 52.182 / 62.332 | 51.722 / 71.367 |
| 显示态 PSS / USS 快照 p50，MiB | 597.293 / 456.016 | 465.045 / 378.926 |
| 收起态 PSS / USS 快照 p50，MiB | 594.568 / 453.871 | 461.991 / 375.832 |
| 显示态 CPU 计数器速率，单核 % | 7.03737 | 0.43149 |
| 收起态 CPU 计数器速率，单核 % | 0.13550 | 0.11881 |

| 顺序 / 渲染器 | 正式代理 p50 / p95，ms | 显示 / 收起 PSS 快照 p50，MiB | 显示 / 收起 CPU，单核 % |
|---|---:|---:|---:|
| A1 Tauri | 52.273 / 62.356 | 596.050 / 593.397 | 7.04575 / 0.12924 |
| B1 MyGo | 61.269 / 81.677 | 464.308 / 461.991 | 0.42524 / 0.11673 |
| B2 MyGo | 51.606 / 71.367 | 466.263 / 461.489 | 0.43774 / 0.12089 |
| A2 Tauri | 52.131 / 62.332 | 600.021 / 596.978 | 7.02899 / 0.14175 |

p50／p95 为 pooled nearest-rank；内存是等捕获次数的经验快照分布，CPU 是两轮增量之和除以实际读取中点跨度之和。原汇总同时保留 first／last counter-read 的区间归一化范围，不能解释为共同时间 CPU 上下界、时钟计数量化界或置信区间。显示态 PSS 中位数约减少 22.1%，但呼出代理尾延迟回退；不据此通过默认迁移或全部性能验收。

显示环境仍为私有 Xvfb 1280×900×24、480×360 应用窗口，固定关闭 WebKit DMABUF／合成及三项 Linux 后台服务。六命名空间、凭据、私有 HOME／XDG／DBus／Secret Service／X11 边界保持；每轮原目标、driver、WM、Desktop pidfd、keeper 及私有服务均按原收据完成清理，errors／forced／remaining 为空。本任务暂停其他本机写入、测试及下载，只是任务调度约束，整台宿主安静、GPU、唤醒、首次交互、搜索／IME／DPI／多屏继续未经验证。

[新机器可读原汇总](../evidence/phase6-linux-process-only-v2.json) 为 92,439 字节，SHA-256 `3c9e9b1d8162859efc11aa5a30cede300b5bf77418559c52f5cb389367807b86`，绑定 136 份闭合原件、11 个实际输入身份、冻结源码和四轮独立原件审查。每轮实际截图均由 Root 查看后才评估；原生资格另由原采样时段的元数据验证，未声称所有资源点都有截图。第二人未导入或执行采样器、评估器及汇总器，从原 performance、八份资源流及八份时钟流独立重算全部分位数与 CPU 区间，结果一致。独立汇总审计为 74,139 字节、SHA-256 `39044e15012b6e4a31e9a97af33fa8260a0edbf456e05225144f2e0ed0b4e04c`；四轮原件及来源前后哈希保持不变。离线一致性不重做原生、隔离、图像或宿主安静验收。

[新公开复算包与入口](../evidence/phase6-linux-process-only-v2/README.md) 提供全部 136 份闭合原记录、58 份控制／评估／截图／审查文件、38 份冻结源码与 11 份 proof／审计，共 243 份原文件及显式路径映射。ZIP 为 2,189,789 字节，SHA-256 `130f8a7c4d728407323d7131e97bb47dc9f89095814daefa3c7666e30e3b2178`。独立汇总审计另有[直接可读原件](../evidence/phase6-linux-process-only-v2/independent-summary-audit.json)。标准库入口只读 ZIP、重算全部正式样本、资源／时钟及汇总，不解压或执行包内代码。七个 ELF 输入只留历史身份及头部元数据，不校验缺席全文；保存的原生、隔离和实际看图收据不等于重新执行这些验收。旧包保持原字节。

## Linux X11 旧源码受限合成配置结果（5d6af007）

本组使用私有 Xvfb 1280×900×24，固定 `WEBKIT_DISABLE_DMABUF_RENDERER=1`、`WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `TAURI_WEBVIEW_AUTOMATION=true`；`window_tracker,edge_docking,mouse_hotkey` 三项 Linux 后台服务由固定环境关闭。保留的命名空间调用记录没有 GPU 设备或 tracefs 绑定。这限定了所测显示、合成和自动化路径，当前 CPU/内存差异不能外推常规硬件合成桌面；GPU 显存与后台唤醒继续保留未测量，不能由这些配置补写为 0。

同一组 release 二进制，顺序 Tauri、MyGo、MyGo、Tauri，各用新配置。每轮 5 次预热、30 次正式可见呼出代理；每种显示状态各 960 次真实资源捕获。全部 120 个正式时间与 7680 个资源快照保留，两侧各有 2 轮。p50/p95 为最近秩分位数；内存为等捕获次数的经验快照分布。CPU 用两轮计数器增量之和除以两轮实际计数器读取中点跨度之和，单位为单核百分比。

| 指标 | Tauri | MyGo |
|---|---:|---:|
| 已预热可见呼出代理 p50 / p95，ms（每侧 60 次） | 52.509 / 62.534 | 51.716 / 65.642 |
| 显示态 PSS / USS 快照 p50，MiB | 603.65 / 456.29 | 472.19 / 380.39 |
| 收起态 PSS / USS 快照 p50，MiB | 600.43 / 454.09 | 468.41 / 376.62 |
| 显示态 CPU 计数器速率，单核 % | 6.4429 | 0.4690 |
| 收起态 CPU 计数器速率，单核 % | 0.1501 | 0.1334 |

各轮分别列出以保留时序和差异，未用有利轮次替换其他轮次：

| 顺序 / 渲染器 | 正式代理 p50 / p95，ms | 显示 / 收起 PSS 快照 p50，MiB | 显示 / 收起 CPU，单核 % |
|---|---:|---:|---:|
| 1 / tauri | 52.509 / 62.848 | 602.65 / 598.79 | 6.3240 / 0.1542 |
| 2 / mygo | 51.480 / 65.642 | 472.84 / 469.83 | 0.4752 / 0.1292 |
| 3 / mygo | 51.834 / 71.751 | 471.41 / 468.41 | 0.4627 / 0.1376 |
| 4 / tauri | 52.162 / 62.303 | 605.90 / 603.45 | 6.5617 / 0.1459 |

显示态 PSS 快照 p50 约减少 22%，CPU 计数器速率约减少 93%。收起态两侧 CPU 都低于单核 0.2%。这些为此环境中两轮每侧的描述性结果；相关快照不是 1920 次独立实验，也没有显著性或长期资源保证。

## 配置、计时与身份

- 目标 `x86_64-unknown-linux-gnu`，每轮内部由同一个 Embedded Host 保持捕获所有权；不同轮使用新 Host 和配置。应用组合包括 Desktop 与其仍运行的子进程、选用的 Quick Paste 渲染器。观察器、Xvfb、窗口管理器及私有 Secret Service 不计入应用组合。PSS/USS 用于避免把共享映射按各进程 RSS 重复相加。
- 源码 `5d6af007b0b000ed78a22e7fc6e1413c92f28d42`；Rust 1.91.1，Go 1.27.3，MyGo 0.6.4 与固定 `mygo-v0.6.4-no-startup-cleanup-v1` 补丁。Desktop 17,432,440 字节、SHA-256 `d28810abb323eef88d1342c4950c25f174f2828657c764db62deb5a990ec6ca9`；Surface 10,723,591 字节、SHA-256 `c97cd7aacdac1e5fe531c64588317ff5dcf46a53744bf42b0ec3557183ca5a8a`。版本串沿用 alpha.5，属于未发布源码实验构建。
- 六个独立命名空间、凭据边界、私有 HOME/XDG/DBus/Secret Service/X11 与原 pidfd 清理保持冻结方案。每轮单独确认安静窗口后执行，测量期间暂停本任务的其他本机写入、测试、构建及产物下载。仅检查已知编译进程与部分系统状态，整台 Windows 主机空闲未经测量。
- 相同 10 条完整普通合成文本、相同 `dzcp6needle7` 查询与唯一完整编号 07 记录、480×360 窗口；原生目标实际收到了完整粘贴文本。两种渲染器使用各自真实界面，保持原原生可见/前台计时终点、10 ms 轮询及 5 s 资格期限。
- 资源先稳定 30 s，然后捕获 960 次；第一个到期点为开始时刻，后续到期点为上次实际开始加 250 ms，无追赶或伪造时间点。末次捕获后保留实际第 960 次开始加 250 ms 的尾部。真实时长、迟到、捕获、序列化、写入与时钟日志成本全部保留。
- 因此内存分布不是固定 240 s 时间平均；CPU 只覆盖实际首末计数器读取区间，排除稳定阶段和尾部。JSON 保留读窗归一化范围，它不是共同时间 CPU 上下界、时钟计数量化界或置信区间。线程集合变化时上下文切换差保持 null；B2 收起态的 6930 为稳定线程集合的原计数差，未换算为唤醒。

## 证据与复核

[提交的机器可读汇总与逐轮原件哈希](../evidence/phase6-linux-count-v1.json) 包含二进制/工具身份、各轮 p50/p95、完整成本摘要、CPU 归一化区间及 136 份原件的角色与 SHA-256。[离线复算包与入口](../evidence/phase6-linux-count-v1/README.md) 另提供全部 136 份原始记录、冻结控制器/计划/评估和分析材料，共 164 份原文件。ZIP 为 4,846,239 字节，SHA-256 `3b2eaa1867b7bfe1fbec4fa7cf18626b279a71f258809ef66215e9facfa4f994`；manifest 保留原字节、原路径与包内相对路径映射。

便携入口只用 Python 标准库直接读取 ZIP，复算 120 次正式延迟、7,680 次资源及其时钟记录和各轮/合并统计，不解压或执行包内程序。整数、纳秒和原件哈希严格相等，派生浮点数使用明确容差。7 份 ELF 输入仅保留原身份，未纳入包；本次离线复算不重新校验这些缺席的实际二进制，不重做原生运行、隔离或目视验收。原本机文件也继续保留；重建原生环境仍需原工具和运行条件。

每轮原件、两份资源流及其时钟流经第二人独立检查，Root 和复核者实际查看了各轮新截取的准备画面。截图只确认查询准备布局；逐捕获原生元数据另外绑定到其采样时间区间，没有为全部资源点宣称截图证据。152 次资格检查对应 244 个实际只读回复；原只读轮询保留同一目标和原时间期限。四轮的冻结停用和清理检查均通过，原 pidfd、keeper、两项私有 Secret Service 和最终无残留检查通过；退出原因和调用方式以原清理记录为准。

完整本机描述性汇总 87,492 字节，SHA-256 `241fa784d832bf67be7406a553fa7e8397c934d1897f9d4a3098276030221c35`。分析脚本 SHA-256 `8f4a342f43e5f52c47a7e373992b680835cb1e68d92980f2ba5ab944fd9bf5f2` 先经源码与纯函数检查，再运行一次。第二人未导入该脚本，从原时间戳、每成员内存和原 CPU 端点以独立算法重算，复核记录 SHA-256 `00c905265201b1034e95736ef9a55ab1e93e506b24a96cc97872ddbb2121e27f`。

旧 v6 在 MyGo 收起态只有 958 个原样本、2 个漏点，仍为原质量门禁失败；停止记录 SHA-256 `d395a9c6feb6c8ca301aa7da8b3a2af7437651f055978fd5b8f4d00fc1cabd29` 保留。此次 count-v1 从头执行不同的资源估计方法，旧样本未拼入或重新标为通过。最终 A2 采用第二人 v2 元数据复核；原 v1 的 MyGo 可见字段标签误用已注明并保留，原采样和 Root 评估未改。

## Windows 与剩余验收

最新[完整运行 41327394886](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41327394886) 的 attempt 1 直接绑定 `2a2521f3a1b04e8bc5c4f44a7d372998aa22577f`，整体成功。现有 `windows-latest` release job 在发布版 13 项功能门禁通过后，于 17:03:57—17:44:54 UTC 顺序执行 Tauri／MyGo／MyGo／Tauri；每轮 5 次预热、30 次正式试次，显示及收起各有 961 个实际样本、约 240 秒观察。总计 120 次正式试次和 7,688 个资源点，没有补点或拼入旧轮。

| 轮次 | 可见／前台代理 p50 / p95，ms | Enter 到完整目标全文 p50 / p95，ms | 显示／收起 WorkingSet p50，MiB | 显示／收起 PrivateCommit p50，MiB |
| --- | ---: | ---: | ---: | ---: |
| A1 Tauri | 18.305 / 29.148 | 269.175 / 277.704 | 240.08 / 64.94 | 207.20 / 207.66 |
| B1 MyGo | 12.465 / 41.548 | 47.703 / 57.416 | 153.63 / 127.21 | 222.85 / 221.96 |
| B2 MyGo | 27.580 / 30.859 | 47.467 / 63.849 | 152.43 / 126.54 | 224.30 / 224.01 |
| A2 Tauri | 16.180 / 26.769 | 274.122 / 293.841 | 222.36 / 64.41 | 207.10 / 208.08 |

两轮每侧的 60 次正式试次按原 nearest-rank 合并描述：可见／前台代理的 Tauri p50／p95 为 17.393／27.303 ms，MyGo 为 27.201／39.606 ms；Enter 到完整目标全文分别为 272.787／288.079 ms 与 47.703／62.302 ms。每侧只有两轮实验；trial 和资源快照不作为独立重复实验，也不推导统计显著性。显示态观察 CPU 为单核 0.352%—0.410%，收起态为 0.072%—0.085%；这是从首次看到的保留进程身份起算的 CPU 计数器范围，未观察到的短命进程和此前 CPU 未计入。WorkingSet、PrivateCommit 分别保留原含义，不能替代 PSS／USS；Windows PSS／USS、GPU、唤醒、首次可交互帧及搜索就绪仍为 null。

原[产物 13094245927](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41327394886/artifacts/13094245927) 为 8,838,796 字节，SHA-256 `2297ed21d15962e5231c5f14e3094ff5861fab47a4e7c7a24d6fe73af5288bad`。134 个原成员、8 份运行源码和 40 路原输出的字节／哈希分别匹配，原帧无 error／overflow／Node kill；原身份、输入／canary、五个 profile 的 bootstrap 与 WebView 策略恢复及清理原件均复核。各 Desktop 的退出 1 为记录的清理终止，不冒称自然退出保证。冻结 v9 评估输出为 `offline_consistency_verified`，`phase6_pass=false`；第二份原资源流与试次数学复算另行确认上述分位数和计数。原 Windows handle／父图没有离线重放；本轮 excluded 分支为空，不能宣称排除分支获得实际覆盖。与 Linux `1fe685c` 相比，`2a2521f` 仅有四个诊断、测试及说明文件变化，产品目录及 Cargo／包清单无差异；不同系统、工具链、二进制及计量方法仍分别归属，不跨平台混样。

以下历史失败及修订继续保留；新完整结果不将它们改写为通过。

[PR #154](https://github.com/qiyuanhuakai/dezirclip/pull/154) 修正验收控制器有界退出、已认证原句柄清理及父子创建代际校验，产品源码和 release 优化参数保持原值。Linux 5d6 与 Windows a11 在 `src`、`src-tauri`、`crates`、`frontends` 及根 Cargo/包清单中的产品文件一致；平台、工具链、build_id 和实际二进制各自绑定，不跨系统混样。

当前 #154 head `bb2f2b28d554d99b66fcbd26d24663fcc7d871bd` 的自身 CI [41295306086](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41295306086)，以及 [#158](https://github.com/qiyuanhuakai/dezirclip/pull/158) head `3381c24b3dd61525eb6fb88449ed9a9be5299713` 的自身 CI [41295317229](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41295317229)，均为 17 项成功、1 项可选跳过；各自当前提交的自动审查无新增意见。旧 CI 的失败保留，不是当前 head 仍失败，也不借此覆盖报告 #156 的独立堆损坏失败。

历史 a11 普通 CI [41100126715](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41100126715) 17 项成功，其 debug 原生 13 项、退出和清理原件已核对。报告的 PR 合并 SHA `54f05234fddae6435c48ee535e12311a5538be65` 与 a11 的 Git tree 一致。保留一次原只读观察内的父快照拒绝：同一原进程由先前运行变为退出 0，后续观察在原期限内接受，没有新输入或失败场景重跑。

新手动运行 [41106374215](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41106374215) 直接绑定 a11；其独立 debug 前置也已 13 项通过。发布版任务于 2026-10-30 18:55:13 UTC 结束失败：`Verify these release bytes before sequential ABBA` 原步骤退出 1，前 12 项通过，第 13 项开头绑定原主窗口的 `observe` 响应 136 报 `Candidate parent PID changed or disappeared between snapshots: pid=7624`。此前响应 134 已记录同一原句柄、创建时间和镜像的 7624 退出 0。该观察位于设置 idle、切换渲染器和隐藏窗口之前，不能当作产品 idle 重建失败；合并报错没有保留两份快照内容，无法硬判 changed 或 disappeared 分支。

本次 supervisor 无超时或终止请求，原 controller 退出 1；Desktop 在清理前仍运行，随后记录清理终止退出 1，16 个已知原进程退出，pending、unconfirmed、remaining 和清理错误均空，bootstrap 已恢复。原件 [13003078312](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41106374215/artifacts/13003078312) 为 156,830 字节，SHA-256 `a8f773cc4526d99f945467dff0cbae306f7a28f28093ade9b4ecf7ff6b74ff4e`；独立原失败复核 SHA-256 `edf9950251fba412c08a33bbb5e96aa425be4b3ae988f2e2a4f08a7e7bbebb22`。没有比较目录、资源流或已开始的 arm，本次为零 ABBA。后续只处理夹具已认证终态进程的观察分支，保留本次原失败。旧 364 的五例 release 功能成功及更早清理前 `0xC0000374` 各自保留；未复现和记录的清理终止不能关闭 heap 根因。

`527c128c59e07267a01ab05ca8cbdd76cf1e1f0e` 已修正该已认证终态对象的观察分支，原 handle/identity 硬校验在例外前，新、pending 和仍运行候选的原有双快照与创建代际检查保持。Root 独立执行 70 项纯逻辑检查通过；新完整运行 [41115292407](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41115292407) 于 2026-10-30 19:26:38 UTC 发起，由现有工作流自身执行普通与发布版功能前置门禁，尚无其发布版四轮结论。原 a11 失败未重跑或改写。

527c 运行随后因独立源码检查发现枚举 API 非 EOF 错误会被当作列表结束，由 Root 主动停止。停止前保存的状态为 14 个成功、3 个运行中，可选比较 job 尚不存在；终态为 14 个成功、4 个取消，后来出现的可选 job 为同一时刻起止、null runner 的取消条目。原 Windows 日志显示取消发生在 cargo check，实际 GUI Desktop 构建、Surface 功能步骤和 ABBA 均无执行记录，不能借用另一次普通 CI 的 13 项成功。独立 operator-stop 复核 SHA-256 `fb958077f8a5fc3830e9c9451a13510c6c4475b884538440e8bcbf458ea78d46` 保留；该方法停止不证明云端实际出现 API、产品或 heap 失败。

`a7676e283319f76457e86262c48475cf56126a61` 只补上 First/Next 的 FALSE 返回值必须对应 `ERROR_NO_MORE_FILES=18` 才是正常 EOF，其他错误保留阶段与原 WinErr 并中止发现。原 snapshot 在 finally 关闭，没有部分 map 认证、枚举重试、输入重试或权限/时间门槛变化；原终态对象分支保持。Root 独立执行 75 项纯检查并核对 AST/派生身份后，发起新完整运行 [41120519234](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41120519234)（2026-10-30 20:05:32 UTC）。它在自身普通 debug 功能前置失败，可选发布版任务未执行，仍为零 ABBA；不能借用另一次普通 CI [41119817463](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41119817463) 的成功。

本次仍为前 12 项通过，第 13 项未完成。原响应 137、141 已符合主窗口销毁条件，141 也符合 MyGo 原生浮层可见且前台条件；按未改动控制器的确定调用顺序，下一次响应 142 的父边拒绝位于第一次 Escape 输入前的观察，尚未发送 Escape。5392 在先前原响应中一直为同一创建时间、镜像且仍运行，最终清理记录同一原对象退出 0。错误文本仍未保存两份父快照，不能断言具体 changed/disappeared 分支，也不能仅由重复进程信息认定当时已有完整认证证书。另保留响应 38 的一次原只读 UIA 8 秒超时，该项随后在原观察期限内成功；未重新输入或重跑失败场景。

原件 [13007769123](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41120519234/artifacts/13007769123) 为 111,369 字节、23 文件，SHA-256 `021b72e883f5146b2d27a1e8e8d42240cc3e6209ac9d1ef78db04f122b2d6126`；独立原失败复核 SHA-256 `5ede421cbbf328bc0160e9d0f7988bef9264192dc2b81253fc5c8c425bbccc35`。supervisor 原退出 1、58.417 秒，无超时或终止请求；18 个已知原对象均退出，pending、unconfirmed、remaining 和清理错误为空，bootstrap 已恢复。本次没有正式比较样本，未以源码修正或清理成功覆盖功能失败。

`9a1d873ae379d017535bc829e93729baf3642000` 将归属规则统一为已完整认证原对象的生命周期证书：每轮校验冻结证书、原 handle/identity 并新读状态与计数，读取失败仍中止；已退出父对象的原句柄也作为发现后代的锚点。仅完整通过双快照及逐边创建代际检查的新候选才批量取得证书；未认证 pending 不进入返回的窗口或输入范围，仍仅关闭且使清理失败。普通 API 枚举错误和其他断言保持闭锁，没有输入、观察重试或时间门槛变化。Root 独立核对冻结源码并执行 83 项纯检查，另两份独立源码审查未发现新增阻断；这不是实际 Windows 通过结论。

新完整请求 [41127251117](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41127251117) 绑定 9a1d873，2026-10-30 20:57:14 UTC 发起。它的 17 项普通前置成功，自身 debug 原生 13 项及发布版原生 13 项也分别经原件复核；发布版复核 SHA-256 `287ee57d90a80a1990fd4873ca65ff635a8a54ec37fb0a807d823775dbe0de34`。发布版响应 37 的一次原只读 UIA 超时保留，该项随后在原期限内通过。5d6 与 9a1d873 的上述产品目录及根清单实际 Git diff 为空；新旧 Windows 原件不拼接，Linux 数值未重新测量。

该请求的对照步骤于 21:34:15 UTC 实际开始，21:34:41 UTC 失败。唯一已开始的是 A1 Tauri：5 次预热完成，首个正式 trial 5 的单次 Enter 后，770 次原目标文本读取均为空，最末距离输入开始约 12,000.7933 ms，全部前台读数仍为原 Tauri HWND。没有接受的正式样本、没有资源窗口；后续 MyGo、MyGo、Tauri 三轮均未开始。顶层 `comparison.arms=[]` 是 await 失败未追加，不能据此声称没有 arm 开始。原件 [13013910989](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41127251117/artifacts/13013910989) 为 428,475 字节，SHA-256 `a5832efe1e4021089a4ba1d6a831b52469fabafdc797f3844247a1d0afbe4c29`，完整原失败复核 SHA-256 `407fa45af31eaccd1c9cabe6e3561f7783180817db5969c29cab5e3fce0e04be`，首正式样本取证复核 SHA-256 `00ac6a3a1c4c5cbb1546ee1fc1f4d565f19b5a88ca864cd440636088fe3c5bee`。

产品日志走到了第六次 copy/shift-insert 分支，但没有保存实际剪贴板值、当时保存的目标 HWND 或焦点 API 返回值，不能用这些日志证明目标收到文本或确定失败原因。输入后的查询结果资格也没有单独观察，只有原固定 700 ms 准备时间。PR #153 的去除粘贴后前端 hide 修正已在本次源码中，相关两个产品文件与该 PR head 的 Git diff 为空，不能再次归因于缺少该修正。A1 的 9 个已知原对象退出、bootstrap 恢复且清理无错误；Desktop 在清理前仍运行，记录的退出 1 来自清理终止，本次没有 heap 退出证据。

另一次普通 CI [41127196755](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41127196755) 的 Surface 13 项通过，但独立 NSIS OwnedUpgrade 任务失败。它仅安装了旧 alpha.1 标签的同源码 debug 夹具；GUI 原进程仍运行，180 次访问其 CDP 端口均拒连，候选 alpha.2 安装尚未开始。实际 GUI 字节匹配旧构建收据且含调试端口常量，仍不足以确定运行时为何没有监听。原包 SHA-256 `58413e81877a9194d9eff6cfe71e44d6973a0721ee5b290432d0e1be42e64708`、独立失败复核 SHA-256 `ba60199ac50fa391d5b8b41ec318ec07d583870ebd787b8787c7d736126b8020` 保留；不把该失败归为已证实的升级替换错误，也不以手动运行的其他成功覆盖。

后续 [PR #157](https://github.com/qiyuanhuakai/dezirclip/pull/157) 将 Windows 粘贴绑定到操作开始时的原外部目标与保留进程句柄，复核实际前台和焦点，并在部分输入失败时保留记录/队列。Service 还需通过能力版本协商，且两阶段粘贴保留原目标，不能在 Finish 时改投新窗口。冻结源码 `0988d5cc072bd46761efb9ebdd8d7954fcdf2f42` 的普通 CI 17 项成功，原生 Surface 13 项和新增 Service 3 个场景分别经原件复核。这次包含产品改动，不能再把先前 5d6/9a1 的产品文件一致结论套用于它；Linux 仍为独立冻结的 5d6 四轮结果，没有重新测量。

该源码的新手动运行 [41156806072](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41156806072) 自身 debug 前置及发布版原生 13 项通过。A1 Tauri 的 5 次预热和 30 次正式粘贴均已完成：每次仅一次 Enter，返回的唯一目标记录全文匹配。随后可见资源流只保存 602 行、首末跨度约 150.249 秒，旧 Native 日志遇到每流 16 MiB 上限，整轮失败；hidden、B1、B2、A2 尚未开始。stdout 丢失响应 1871，却仍保存较小的 stop 1872，因而它也不是完整前缀。不能把 35 次成功粘贴当作完整性能轮次，更不能将部分资源窗口补成完整 240 秒。原件 [13026307793](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41156806072/artifacts/13026307793) 为 1,227,622 字节，SHA-256 `a71af548c911888403cadd642b9be2ae58c9a3b546eed7c42f2805ffaf5d9d43`；原失败复核 SHA-256 `a04efc394442bb51fecbc526ee5f9e3dce4881c09009c5fac2c5c7bef50f686f`。本次已知原进程全部终结，Native 退出 0，无清理前 heap 退出证据，但日志上限错误使整组不合格。

[PR #158](https://github.com/qiyuanhuakai/dezirclip/pull/158) 的 `0702748ea7a09f9b5a02cdbcd64b02374c5d2476` 只替换派生性能观察器的日志保存方式：stdout/stderr 原字节流式落盘，每轮合计 128 MiB、单帧 1 MiB；首次写入/解析/配额错误永久标记失败，分别核对接收与落盘计数、摘要、关闭和回读。240 秒、250 ms、5 次预热、30 次正式输入及四轮顺序保持原值，原功能和 Service 控制器不变。9 项 Python 与 24 项 Node 纯检查在本机及该源码普通 CI 分别通过；这些纯检查不等于实际 Windows 流式观察通过。该普通 CI 的 Surface 13 项和 Service 3 项成功，但独立 NSIS OwnedUpgrade 在主 WebView 调试发现阶段失败，整次 CI 失败，不能称所有检查通过。

070 的新手动运行 [41163584925](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41163584925) 自身 Surface 13 项通过；Service 3 个场景也完成，但其 Native 清理退出 1，所以整个 Service 门禁仍失败，可选发布版/ABBA 任务被跳过。首次错误为候选 2036 的创建时间早于当前父 PID 2028：分别为 `134378901727430219` 和 `134378908812181480`（Windows FILETIME 原整数）。原 Service 10012 与 Desktop 4232 的创建时间分别为 `134378908804920500`、`134378908812120493`，2036 比两个原始产品根都早约 708 秒。旧校验正确拒绝了这个父代关系；新夹具修复尚须明确证明全部原根及负归属边界，不能宽泛忽略父代错误。

该次 9 个新打开但未认证的候选句柄仅被关闭，未取得归属证书、未被终止；不能从一个候选的时间证据推断其余 8 个的归属或安全终态。两个已认证原产品根通过原句柄清理终止退出 1，supervisor 无超时/终止请求，但 `discovery_complete=false`、`remaining_owned_processes=null`，bootstrap 与 scoped policy 未恢复。原件 [13029000656](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41163584925/artifacts/13029000656) 为 33,420 字节，SHA-256 `b3ec14da6f10d58368504cdb438efc8f94613f9a8596e90d88f85959c07c233e`；独立原失败复核 SHA-256 `38afcc237f03f59e43f929be3fcb1f614e8af7fef66af405216f90c9f26553ed`。这次没有实际执行新的流式性能观察，也没有新的产品粘贴、资源或 heap 失败结论。

随后 [PR #159](https://github.com/qiyuanhuakai/dezirclip/pull/159) 修正安装夹具的 WebView 参数及两项已知环境变量恢复；[PR #160](https://github.com/qiyuanhuakai/dezirclip/pull/160) 增加有界负归属分类：只有新打开、尚未认证、严格早于全部原产品根及旧认证锚点的候选，才能在完整校验后仅关闭句柄。API 失败、仍不合格的父边及关闭失败继续中止，已认证证书不被删除；FILETIME 的 UTC 创建顺序仍是既有父链规则的前提，未证明宿主时钟从未调整。

当时的候选 `3cc1513f4bdcdaad348d0918d0d2565c07abe84b` 的普通 CI [41278991285](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41278991285) 已结束：12 项成功、5 项失败、1 项可选跳过。自身 Windows 后端的 Surface 13 项与 Service 3 项均通过，原件的 Native 退出 0 且 stdio 已关闭，归属清理的 pending、unconfirmed、remaining 和错误均空。本次两包的排除列表为空、仅关闭候选计数为 0，不能称已在实际运行中触发新的负归属分支。

五项失败是四个 NSIS 场景未发现主 WebView，以及 ARM64 Flatpak 在 OCR 英文、中文检查通过后等待隔离 Xvfb 就绪超时，尚未执行粘贴验收。四项 NSIS 均已进入主 WebView 发现阶段；该次运行已复核的 LegacyUninstall 与 OldToNew 原收据均记录 PowerShell 7.6.7 下 12 个环境状态／正常及合成异常场景全通过，不能替其他任务出具同一结论。Xvfb 原脚本把 stderr 接入 PIPE，却未读取或输出，启动根因仍未确定。这次没有发起手动发布版对照，不以普通 Windows 功能成功覆盖整次 CI 的失败。

后续 [PR #161](https://github.com/qiyuanhuakai/dezirclip/pull/161) 为 Xvfb 保留直接 stderr、单次启动的原 15 秒就绪期限及有界原进程清理。其原提交 `c3943ae` 的 CI [41283880025](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41283880025) 中，x64 和 ARM64 最终 Platform OCR／粘贴门禁均通过；历史 Xvfb 超时原因仍未知。同一次 CI 的四个 NSIS 场景失败，Surface 第 9 项受保护搜索未完成；原件记录 MyGo helper 4604 退出 2，未记录对它的清理终止请求，整轮仍失败，不能据此认定完整候选合格。

该 Surface 失败又暴露诊断问题：未找到匹配的可见 Surface 时 Python 发出空错误字符串，Node 按真假判断而错误地接受回复，随后报 `elements` 异常。原始子进程退出、空错误回复和完整归属清理证据分别保留。[PR #162](https://github.com/qiyuanhuakai/dezirclip/pull/162) 明确拒绝所有带 error 字段或缺 value 的回复，并保留合法空值；作者、Root 与独立复核者各完成 116 项纯检查。这只是诊断修复，尚未证明 helper 的实际退出根因。另查到 Panel 原地清零列表状态、旧 MyGo 渲染帧仍引用它的可达风险；[PR #163](https://github.com/qiyuanhuakai/dezirclip/pull/163) 改为替换列表状态实例，并为渲染及提交回调增加可撤销快照、代际与记录 ID 校验。撤销清除快照已存文字，但不承诺主动清洗上游已经物化的渲染／UIA 树。

安装夹具的后续提交 `5374742` 复用既有可恢复的 WebView 权限配置事务。自身 CI [41285988531](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41285988531) 为 17 项成功、1 项可选跳过，当前源码自动审查无新增缺陷。四个 GUI NSIS 场景的五个阶段原件均独立核验：实际高权限配置、原进程身份、CDP、旧历史、相应捕获及配置恢复通过。该结论只属于自身源码与运行；#154 和 #158 尚未包含该修复时的 OwnedUpgrade 失败继续保留，不能借后续成功视为旧 CI 全绿。

另一旧候选 `96058c13ac2eaacc4b6934c9361349ca92fa4b61` 的自身 CI [41286629791](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41286629791) 中，Windows x64 的 Surface 前 8 项通过，第 9 项在显示主窗口时未完成。原 Desktop 8336 在清理前退出 `0xC0000374`，未记录对它的 kill／forced 终止；这与 `c3943ae` 的 MyGo helper 4604 退出 2 是不同原进程、不同失败。原件 [13076740430](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41286629791/artifacts/13076740430) 为 59,601 字节，SHA-256 `b6dc5fd32d148628a8743209c9fecbc78c68c74841314e18a2d14bc0217fb8ee`。该堆损坏记录不能改写为正常清理退出，根因仍未关闭。

包含列表帧修复的 `73e1d1a820fc1914c3b7ffb9e4d6cd6ab9789031` 自身 CI [41289634097](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41289634097)（attempt 1，报告合并提交 `5e17ea74fd19bf25fb3140baf138829dc21c52e6`）为 17 项成功、1 项可选跳过。自身 Surface 13 项、Service 3 个场景及四个 GUI NSIS 场景的五个阶段原件分别复核通过。新增 9 个纯模型测试函数位于现有 `go test ./...` 的覆盖范围，该源码的现有 Go 测试步骤通过；这与前述 116 项 Python／Node 纯检查分别归属，未在本机执行 Go 或原生程序。这次成功不证明 Panel 风险就是 helper 4604 的实际退出原因，也不关闭 Desktop 8336 的堆损坏根因。

为使 #154 和 #158 自身包含安装夹具修复，已将 #159 的三次提交从 `0702748` 后前移到 #151 的 `f7ef5ab` 后，再顺序重排 #154、#157、#158、#160、#161、#162、#163。各 PR 改动范围与 23 次提交的差异核对均保持一致；该次重排的 #163 为 `b62476fb217cb3141f02df5cfdd91e61681d8c4b`，完整 Git tree 与原 `73e1d1a` 相同。重排未将旧失败改写为通过，各新头按自身运行归属。

首次重排后，#154 的自身 CI [41291290029](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41291290029) 中，Linux x64 实际构建、测试、兼容性及挂载检查均通过，但 Rust 缓存收尾执行 `cargo metadata --all-features` 时失败，runner 随后记录磁盘耗尽。该 job 的 Rust 缓存未命中，失败前尚未开始 Rust 归档，不能将根因断定为缓存恢复或归档峰值。修复仅在该 Linux x64 CI 中设置 dev/test 的 debug 信息为 0，保留全部检查、Windows、NSIS 和 release 配置，并记录缓存前、主检查后两个固定目录的磁盘及 inode 元数据；这两个时点不覆盖 post-cache 峰值。改动接入后的 #163 为 `b39d93f4b6dc05be127be2361faff7b70908ad26`，重排后的 8 个新头各自普通 CI 均为 17 项成功、1 项可选跳过；未受调整的 #159 `c098011` 自身 CI [41291146886](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41291146886) 也为 17 项成功。#157、#158、#160、#161、#162、#163 当前提交的外部自动审查已于 11:36—11:37 UTC 明确无新增意见。现有证据仍无法确定旧磁盘故障的完整原因，也不产生性能结论。

唯一新手动运行 [41302851457](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41302851457) 绑定 `b39d93f`，整体失败。其 debug Surface 的 13 项功能检查通过，但原响应 136 在打开 PID 6704 时收到 AccessDenied：请求权限 `0x101011`，没有保留原句柄。随后 3 个未认证候选只关闭句柄，发现不完整、剩余对象无法确认，清理失败；不能补推这些 PID 的身份或归属，也没有本次 heap 失败的证据。

同一运行的独立 release 诊断完成 5 个新配置、65 项功能检查，结果为 `not_reproduced_5`。125 份元数据的原件／副本哈希、40 份原始输出日志以及源码绑定、WER 和配置恢复分别复核通过，没有 dump，也没有 ABBA。元数据包未包含实际 PE 字节，不能重新计算二进制哈希；各 Desktop 的退出 1 均为计划清理终止，不能冒充自然退出保证。五次未复现仍不关闭历史堆损坏根因。

[PR #164](https://github.com/qiyuanhuakai/dezirclip/pull/164) 的 `a4f098e164fb150aae321dd8340f58ec45661d2e` 仅在 5 个夹具／测试／说明文件中移除当前 API 未使用的 `PROCESS_VM_READ`，没有产品改动；Root 117 项、独立复核者 78 项纯检查及源码审查通过，当前提交自动审查 `6558075233` 无新增意见，自身 CI [41307309602](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41307309602) 为 17 项成功、1 项可选跳过，其 Surface 13 项、Service 3 项及 24 份原始输出流分别复核通过。这不证明 PID 6704 权限拒绝或 heap 根因已修复；旧 Windows assessor-v5 仍绑定 `b39d93f`，最终新源码需要新的完整 cohort 与绑定。

[PR #165](https://github.com/qiyuanhuakai/dezirclip/pull/165) 的首个提交 `3c9f5e65760f153b7035bf9d4e6d1c487cb81d40` 在 #164 之上增加 4 文件、默认关闭的 Linux release 输入供给。其 CI [41313186568](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41313186568) 的 Frontend 任务 [124867667311](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41313186568/job/124867667311) 在 Python 3.10 导入 `tomllib` 时失败，该新增纯测试与供应器均未执行。修订提交 `1fe685c424602b2bc1892f1deb0794c2e0b90ad5` 仅为该测试固定 Python 3.13.15 并更新合同／说明，供应器保持不变；新提交自身 CI [41314890080](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41314890080) 已终态成功，17 项成功、1 项可选跳过。唯一新手动运行 [41317927879](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41317927879) 的 Linux x64 供给步骤和整个 producer job 成功；原产物 `13088123731` 为 12,133,498 字节，SHA-256 `1098ba1a989db9dd42d2728b7101f582d2683e414aad943f15e132aa8311a0f0`。13 个原成员、11 份清单校验、973 行 Cargo 原 JSON、两个 ELF 与原 proof 分别核验；Rust/Cargo 为实际 1.99.1，Go 为 1.27.3。Desktop proof 明确记录 GHA 构建及 Cargo artifact 身份绑定、producer 未执行 Desktop，不冒称旧命名空间离线构建收据。Linux count-v1 的已有数值仍只属于 `5d6af007`，不归属新增 MyGo O(n) 帧快照后的源码。

新 `1fe685c` release pair 已完成一次原有 19 项 Linux 功能验收，Root 与第二人分别读取原件复核通过；31 份原记录、91 次原生观察、87 个目标事件及 39 个目标回复分别核验。MyGo 的单次 Return 产生一轮原目标 Ctrl+V 与一次 SelectionNotify，Tauri 产生一轮 Shift+Insert 与一次 SelectionNotify，两者均交付同一 row7 全文；热复用、真实隐私 epoch、焦点恢复、Host 捕获所有权与原对象清理通过。六命名空间、私有凭据、HOME/XDG/DBus/Secret Service/X11 边界保持原值。启动前的首个授权助手因已有 Xvfb 名称停止，未生成 GO 或执行 cohort；经独立审查的新助手只将 Xvfb/Marco 记为来源未认证且未操作的既有显示服务，未认证整机空闲，Windows 进程／空闲字段为 null。

同一新 release pair 的原 R3 13 项随后单次执行成功，Root 与第二人独立读取 25 份原记录复核：两个静默区间为 35.154086779 与 35.006903618 秒，各 141 次原对象观察；43 次完整 driver 事件均不与静默区间重叠，调用计数分别保持 34→34 与 39→39。原 helper 的 PID／创建代际／会话／XID 不变；一次 Enter 只产生一次 Ctrl+V 和 SelectionNotify，并交付 row7 全文。10 条历史、捕获所有权、隐私 epoch、正常停用的原 helper pidfd 退出和最终私有对象清理通过。原授权助手的工具完成输出曾丢失，因此其退出状态仍为 unknown；另经源审和 18 项纯模拟检查，以新 Root 单次决定对既有三份授权文件执行只读持久化／回读及新观察，保留原件后才单独启动一次原 R3，未重跑旧助手。新的恢复入口及实际 outer 均保留退出 0；这不补推旧助手退出状态。19 项和 R3 均为功能范围，不认证整机安静、性能或 Phase6。

新 `1fe685c` count-v1 的单次 A1 Tauri 随后失败，余下三轮均未启动。原 5 次预热、30 次正式试次及显示态 960 次采集完成；收起态第 460 次调用在读取 `/proc/68/task/546/stat` 时收到 ENOENT，只有 459 条完整样本／时钟流记录。原 progress 的第 460 条仅有 due／start／late，没有 capture／序列化／写入终点；不作为完成样本。首错为 `process sampling failed stage=thread-stat pid=68 tid=546`，该路径不存在本身不足以证明整个 Desktop 退出或正常线程退出。原 outer 退出 1，整轮保持 failed，没有补点、重跑或合成完整四轮结果；当时已有 Linux 性能表只归属旧 `5d6af007`。原目标退出 0、driver 记录 -15、WM 退出 0、Desktop pidfd 退出及清理 errors=[]；keeper 的 child 退出 1、forced／remaining 均为空，私有 Secret Service 的 command 退出 1 也继续保留为本次子任务失败，不能把整个收据改成通过。独立失败审计复核 228 份原文件、47,805,016 字节及 47 项来源／输入／前提记录，前后哈希均保持；1,419 条已写样本与原时钟逐项匹配，第 460 次失败捕获未算入。后续另经方法冻结及独立审查，从头执行的 process-resource-only-v2 完整四轮见文首；本次部分数据没有贡献到新汇总。

同一手动运行的 Windows 普通前置及 release 13 项功能门禁通过，但第三轮 MyGo 仅完成 5 次预热和 29/30 次正式试次，尚无该轮资源数据；原 Native 响应 855 报 `Owned image unavailable`，第四轮没有 report。原 ZIP `13090360051` 为 5,633,793 字节，SHA-256 `64d29a1819b04b5d3fe0614d4849d52fe8cadf194055fa7d1a12d8713dfbe3f4`；32 路原日志字节及哈希完整、没有丢帧或 overflow。唯一未认证清理 PID 5404 只关闭句柄，没有其创建时间、映像或父边，不能补推为已认证错误对象。该轮发现不完整、剩余对象为 null，bootstrap 与 policy 恢复收据缺失；Desktop 6972 的退出 1 在清理开始后且属于强制列表，不能断言为堆损坏，也不能认证正常恢复。冻结 v7 离线评估拒绝这份 2 个已发布通过轮次、3 份发现 report 的不完整 ABBA；前两轮数值不作为完整新结论发布。

[PR #166](https://github.com/qiyuanhuakai/dezirclip/pull/166) 的 `2a2521f3a1b04e8bc5c4f44a7d372998aa22577f` 仅补 Windows 原映像失败的有界错误上下文、派生绑定及模拟测试，未新增原生查询或放宽身份、权限、采样及失败规则。Root 73 项目标、10 项派生和 13 项 Node 纯检查通过，独立目标／派生 AST 复核通过；首提交 `7923f73` 的五个 Linux CI 失败均为同一既有纯测试漏模拟 Windows 错误读取接口，保留原失败。修订仅补该受控模拟，双方在移除 Windows accessor 的条件下各自 73 项通过；自身 CI [41326398315](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41326398315) 为 17 项成功、1 项可选跳过，当前提交的自动审查 `6560479435` 无新增确定问题。自身 Surface 13 项、Service 3 项和 24 路原输出流独立核验通过。新的[完整同源对照 41327394886](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41327394886) 及其原件已完成，冻结 v9 的 20 项纯测试、8 个源码绑定、原算法及完整实际产物分别复核，结果与限制见本节开头。

报告旧提交 `3956cb6106ecc3336d67d679a686f032f206d079` 的自身 CI [41338558022](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41338558022) 为 16 项成功、1 项失败、1 项可选跳过，不能沿用前次 CI 的绿色状态。Windows x64 原生门禁在第九项 MainShow 失败，实际 Desktop PID 1584、创建时间 `134380336864210807` 于 19:08:13.861 UTC 在清理前退出 `3221226356 = 0xc0000374`，比 19:08:34.134 UTC 的清理早 20.273 秒；原句柄已记录同一异常退出，kill 请求为 false。此前八项通过，上一项粘贴全文在 19:08:13.593 UTC 已通过；这次尚未进入 idle destroy／recreate，不能据此归因为销毁重建。11 份原证书对象终态及配置恢复完成，不改变产品提前崩溃的事实。原 WER reader 约 8.025 秒超时后被终止，没有模块偏移、转储或符号栈；具体原因仍未定位。

该失败原[产物 13097072391](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41338558022/artifacts/13097072391) 为 59,547 字节、SHA-256 `d784513b72817e2c2b1f738d61fd0cbe6ea35359e7dd72eab28ac91df960d02b`；独立原件审计为 11,459 字节、SHA-256 `6e5f6681b39ac9bacbb8a2948887181f807c1e2793eeff40febee398643b85ce`。旧报告与基线的产品、工作流及构建清单差异为空，失败保留，不将它解释为文档修复或新性能结果。

[PR #167](https://github.com/qiyuanhuakai/dezirclip/pull/167) `e751e70da17a4304636a3c223777a43a26e1c30d` 增加默认关闭的 debug 诊断选项，保留实际 EXE、PDB 及同源 build receipt。只在诊断构建步设置 dev debug=2／strip=none，release、ABBA、产品及原身份／WER／恢复规则不变。Root 与独立复核者各自 48 项纯模拟检查通过，当前提交自动审查 `6563027536` 无新增确定问题，自身普通 CI [41343649947](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41343649947) 为 17 项成功、1 项可选跳过。

唯一新 hosted debug 诊断 [41343726885](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41343726885) attempt 1 直接绑定 `e751e70d`，整体 18 项成功；五个新配置各 13 项检查通过，结果 `not_reproduced_5`，没有执行 release／ABBA。原 metadata [13099852050](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41343726885/artifacts/13099852050) 为 862,214 字节、SHA-256 `4ee0bef03242944b4e0387219324e31cda59a833bdcc547646216a56c7b15087`，符号 [13100016520](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41343726885/artifacts/13100016520) 为 93,767,258 字节、SHA-256 `2621e50b9be12ff05ae5412853b44a34ea4b116b088a1305dc0aafebde82d671`。独立复核两份整包及其中 162／3 个成员的字节、SHA 和 CRC；125 份保存元数据、40 路原日志、五例输入前后链、WER 及配置恢复分别匹配。仅末例 25 份原路径文件同时留在包内，不声称同时取得全部 125 对原路径文件与副本。

实际 EXE 49,565,696 字节、PDB 328,085,504 字节，均与 2,543 字节 build receipt 及五例前后哈希链一致。五个 Desktop 的退出 1 属于记录的清理强制终止，不能称自然退出；本次没有清理前异常或 dump 候选，未加载符号、未形成符号栈，EXE／PDB 签名匹配仍为 unverified。新诊断链实际走通与五次未复现不关闭 `3956cb6` 或更早 heap 根因，也无法补建历史转储。

首次冷启动、首次可交互帧、搜索/键盘延迟、GPU/显存、后台唤醒、IME、DPI 与多屏仍未完成同口径对照。当前完成的 Linux 新旧及 Windows 四轮均限于所述原生窗口代理、合成配置和各自资源范围；没有认定全部性能要求或故障隔离通过。任何夹具源码或方法变更都需新完整四轮，旧失败原件不改写、部分样本不混合。Phase 6 和 Phase 7 继续保留未通过/未启动状态。

## 补测方法与环境条件

已做只读源码、原件和工具文件库存调查，没有据此启动新原生采样、安装输入法、修改宿主显示或提升权限。后续每项都需独立固定方法、能力和输入，原实验快照不能补写为这些指标。

| 项目 | 可准确测量的边界与必要条件 |
|---|---|
| 新进程启动 | 启动前固定渲染器和数据，由同一观察器记录 spawn 前后单调时钟，直到实际热键/查询/单次 Enter 后独立目标收到完整文本；名称使用 fresh-process-to-verified-action。现有流明确为 already-started，输入哈希预读也已暖文件缓存，不能回填为进程或磁盘冷启动。 |
| 粘贴与搜索/键盘 | 单次 Enter 到独立目标完整文本可用作粘贴验证延迟；一次末字符输入到完整查询、唯一编号 07 记录，以及一次方向键到实际选中状态，记录 last-negative/first-positive 和读取区间。DOM/UIA/AT-SPI 只读状态仍是 observed-state 指标，新增可访问性读取会影响 MyGo 惰性描述缓存，需新配对 cohort。 |
| 首次可交互帧 | 目前缺少两端同义的内容呈现、可处理真实输入及其因果关联契约。NativeVisible、UIA、DOM、rAF 或 WaitForInputIdle 不能单独关闭该要求；Xvfb 也不提供物理 scanout。继续保留未验证。 |
| 显存/共享映射 | 先确认实际设备与支持字段。DXGI QueryVideoMemoryInfo 只描述调用进程，适配器容量也不是目标用量；DRM fdinfo 字段可选，跨进程/client 的共享不能简单求和为唯一物理显存。当前记录不具备硬件计量证据，CPU PSS 不替代 VRAM。 |
| 后台唤醒 | 研究带原线程生命周期、完整单调时间区间和丢事件计数的 ETW ReadyThread / Linux sched_wakeup；定义换入、wakeup_new 等事件语义，按目标 TID 归属。context-switch 不替代 wakeup，六命名空间内 root 不代表宿主 perf/trace 权限。缺能力或数据丢失须标记 unavailable/incomplete。 |
| 中文 IME | 当前原生目标输入只覆盖 ASCII；UNICODE/VK_PACKET 不是组合输入。需固定真实中文转换引擎、输入框架、字体、私有总线与候选窗，实际验证 preedit/候选、Enter 只提交、Esc 先取消、最终中文单次粘贴及焦点/隐私。只读库存有 IBus 框架和 CJK 字体，未发现已列 manifest 中的中文转换引擎；未启动或修改用户 IME。 |
| DPI/多屏 | 现有窗口矩形不等于 DPI 或 monitor 验收。需记录 awareness、实际 monitor/workarea/DPI/逻辑和物理矩形并区分镜像伪监视器，再验证真实混合缩放跨屏移动、候选窗/插入点/点击/焦点。更改 Xft.dpi 或发送模拟 WM_DPICHANGED 只可称布局代理。 |

相关原始契约：[MyGo 固定版本可访问性](https://github.com/egoist/mygo/blob/6255ea40139d06e0762a7c4a32626148ba410478/docs/ui/accessibility.md)、[DXGI 调用进程显存](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgi1_4-idxgiadapter3-queryvideomemoryinfo)、[DRM usage stats](https://docs.kernel.org/gpu/drm-usage-stats.html)、[ReadyThread](https://learn.microsoft.com/en-us/windows/win32/etw/readythread)、[perf 权限](https://docs.kernel.org/admin-guide/perf-security.html)、[Text Services Framework](https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework)、[GetDpiForWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow)、[EnumDisplayMonitors](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaymonitors)。这些为补测依据，不构成实际执行通过。
