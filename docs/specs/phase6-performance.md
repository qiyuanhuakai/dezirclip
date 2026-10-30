# Phase 6 MyGo / Tauri 性能对照记录

日期：2026-10-31。Phase 6 尚未通过，Phase 7 未启动，默认仍使用 Tauri。

Linux X11 的新四轮受控对照已完成并独立复算。MyGo 在此配置下减少应用进程组合的内存及静态显示 CPU；已预热窗口呼出到原生可见且前台的 p95 从 62.534 ms 变为 65.642 ms，增加约 3.11 ms。此计时终点是原生窗口代理，首次可交互帧、搜索就绪等仍未测量；没有据此认定默认迁移门槛或 Phase 6 验收通过。[完整要求](v0.2-mygo-ready.md#p66-性能评估)

Windows 本次发布构建对照 [41106374215](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41106374215) 已结束失败，源码为 `a11cb83fab6e057d18301c05dca6cb3e3ce0a83f`。发布版功能前置 12 项通过，第 13 项开头的原生观察被夹具拒绝，零 ABBA 轮次，尚无 Windows 正式性能结论。

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

[提交的机器可读汇总与逐轮原件哈希](../evidence/phase6-linux-count-v1.json) 包含二进制/工具身份、各轮 p50/p95、完整成本摘要、CPU 归一化区间及 136 份原件的角色与 SHA-256。原始流和冻结控制器在本机 `.harness/phase6-linux-count-observer-v1-5d6-20261030` 及对应 Linux 缓存目录保留；提交的 JSON 是描述性派生记录，重建原生环境仍需这些输入。

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

`a7676e283319f76457e86262c48475cf56126a61` 只补上 First/Next 的 FALSE 返回值必须对应 `ERROR_NO_MORE_FILES=18` 才是正常 EOF，其他错误保留阶段与原 WinErr 并中止发现。原 snapshot 在 finally 关闭，没有部分 map 认证、枚举重试、输入重试或权限/时间门槛变化；原终态对象分支保持。Root 独立执行 75 项纯检查并核对 AST/派生身份后，发起新完整运行 [41120519234](https://github.com/qiyuanhuakai/dezirclip/actions/runs/41120519234)（2026-10-30 20:05:32 UTC）。当前尚无其发布版四轮结果，原运行未重跑、原数据未重新标绿。

首次冷启动、首次可交互帧、搜索/键盘延迟、GPU/显存、后台唤醒、IME、DPI 与多屏仍未完成同口径对照。当前仅证明所述 Linux 窗口代理与资源范围；Windows 旧运行均未进入正式计时，新 a767 完整运行尚待结果。Phase 6 和 Phase 7 继续保留未通过/未启动状态。

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
