# Phase 6 MyGo / Tauri 性能对照记录

更新日期：2026-11-01。Phase 6 尚未通过，Phase 7 未启动，默认仍使用 Tauri。

Linux X11 冻结源码 `5d6af007` 的四轮受控对照已完成并独立复算。MyGo 在此配置下减少应用进程组合的内存及静态显示 CPU；已预热窗口呼出到原生可见且前台的 p95 从 62.534 ms 变为 65.642 ms，增加约 3.11 ms。此计时终点是原生窗口代理，首次可交互帧、搜索就绪等仍未测量；结果不自动归属后续产品源码，没有据此认定默认迁移门槛或 Phase 6 验收通过。[完整要求](v0.2-mygo-ready.md#p66-性能评估)

Windows 尚无完整且可接受的四轮对照。重排后的 8 个新头及权限夹具 `a4f098e` 的普通 CI 均成功，但 `b39d93f` 的唯一新手动运行整体失败；其中独立 release 诊断完成五例、未复现堆损坏，没有执行 ABBA。当前供应器候选 `1fe685c` 自身 CI 尚待结果，旧 MyGo helper 退出 2、Desktop 清理前堆损坏及本次权限拒绝的根因均未关闭。各次失败和部分原件分别保留，未拼接成完整结果。

## Linux X11 受限合成配置结果

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

[PR #154](https://github.com/qiyuanhuakai/dezirclip/pull/154) 修正验收控制器有界退出、已认证原句柄清理及父子创建代际校验，产品源码和 release 优化参数保持原值。Linux 5d6 与 Windows a11 在 `src`、`src-tauri`、`crates`、`frontends` 及根 Cargo/包清单中的产品文件一致；平台、工具链、build_id 和实际二进制各自绑定，不跨系统混样。

当前 a11 普通 CI [41100126715](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41100126715) 17 项成功，其 debug 原生 13 项、退出和清理原件已核对。报告的 PR 合并 SHA `54f05234fddae6435c48ee535e12311a5538be65` 与 a11 的 Git tree 一致。保留一次原只读观察内的父快照拒绝：同一原进程由先前运行变为退出 0，后续观察在原期限内接受，没有新输入或失败场景重跑。

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

[PR #165](https://github.com/qiyuanhuakai/dezirclip/pull/165) 的首个提交 `3c9f5e65760f153b7035bf9d4e6d1c487cb81d40` 在 #164 之上增加 4 文件、默认关闭的 Linux release 输入供给。其 CI [41313186568](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41313186568) 的 Frontend 任务 [124867667311](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41313186568/job/124867667311) 在 Python 3.10 导入 `tomllib` 时失败，该新增纯测试与供应器均未执行。修订提交 `1fe685c424602b2bc1892f1deb0794c2e0b90ad5` 仅为该测试固定 Python 3.13.15 并更新合同／说明，供应器保持不变；新提交自身 CI 尚待结果，真实供应步骤仍未执行。Linux count-v1 仍只属于 `5d6af007`，新增 MyGo O(n) 帧快照后不能归属当前源码；当前还缺 release Desktop／Surface 原输入和各自 proof，以及新源码的 functional 19 项、idle 13 项资格。普通 CI 的 CLI／Flatpak 产物不能替代，当前源码的新 prerequisites 与 ABBA 均未执行。

首次冷启动、首次可交互帧、搜索/键盘延迟、GPU/显存、后台唤醒、IME、DPI 与多屏仍未完成同口径对照。当前仅证明所述 Linux 窗口代理与资源范围；Windows 尚无完成且可接受的性能对照。任何夹具源码或方法变更都需新完整四轮，旧失败原件不改写、部分样本不混合。Phase 6 和 Phase 7 继续保留未通过/未启动状态。

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
