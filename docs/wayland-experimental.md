# Wayland Clipboard Experimental

Wayland 剪贴板后端与窗口 Surface 是独立能力。后端使用原生 Unix socket 和官方生成的 Wayland 协议绑定；不启动 `wl-copy`、`wl-paste`、`xclip`、`xsel` 或 `xdotool`，不以 XWayland 连接代替原生 Wayland 能力。

## 后端与所有权

`dzc_platform::wayland::WaylandClipboardBackend` 首先连接实际会话、枚举 registry，再优先绑定 `ext_data_control_manager_v1` v1；只有缺少该接口时才绑定 `zwlr_data_control_manager_v1`（最高 v2，CLIPBOARD 也支持 v1）。不使用需要 Surface 焦点的 `wl_data_device` 作为后台降级。

报告包含实际 advertised/bound 协议版本、seat global/name/version 和 data device 完成往返协商的状态。默认选择 global ID 最小的 seat；多 seat 用户可通过 `DEZIRCLIP_WAYLAND_SEAT` 指定实际名字。协议没有提供来源应用身份，因此不将当前 X11 活动窗口当作 Wayland selection owner。

`permission=data_device_negotiated` 表示本次创建 data device 和往返已经成功，不保证权限永久有效。设备 `finished`、选中 seat/manager 被移除、断线或协议错误会结束监听。连接失败、缺协议、无 seat 和权限撤销分别返回稳定错误，不回退到 XWayland，也不包含正文或环境变量原文。

后端只处理 CLIPBOARD。每次 selection 保留其原生 offer 和 MIME 清单，读请求始终使用同一个 offer，不在不同 owner 之间拼接正文。监听只向适配层传递递增序号；需要的正文由捕获读取器按需获取。KDE `x-kde-passwordManagerHint` 可以先独立读取，最多64字节；没有该 MIME 与读取失败是不同状态。

传输使用有界 pipe、非阻塞 I/O 和三秒上限；单次读取/写入总预算64MiB，最多128个 MIME，每个 MIME 名最多256字节。来源未应答、超时、超限和不支持的 MIME 均明确拒绝。写入就绪回调在合成器处理 `set_selection` 的往返之后执行；供数循环持续到所有权被替换、显式取消或连接断开，调用返回不代表可以立即销毁 selection owner。

已研究 [wl-clipboard-rs0.9.4](https://docs.rs/wl-clipboard-rs/0.9.4/wl_clipboard_rs/)。其 watch offer 借用整个 watcher，copy 的 prepared owner 无取消句柄，不能直接满足共享捕获 Host 的有界停止/确认就绪契约。本后端复用同系列 `wayland-client` 和官方生成的 [ext-data-control XML](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-data-control/ext-data-control-v1.xml) / [wlr-data-control XML](https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-data-control-unstable-v1.xml)，不复制协议 wire 编解码器。

## 可取消捕获订阅

`capture::subscribe_clipboard(callback)` 返回拥有 worker 的 `ClipboardSubscription`；调用 `wait_ready(timeout)` 后才能认为监听已经激活。启动失败/超时仍保留 handle。

`stop()` 通知原生循环并等待清理、join，最多等待五秒。没有确认退出会返回 `ClipboardStopNotConfirmed`，保留 handle 供再次停止；捕获 Host 在此期间必须继续持有捕获所有权锁。`is_stopped()` 只在实际 join 后为真。Drop 尝试停止，不能替代 Host 的显式确认和持锁顺序。

- Windows：仅给本订阅拥有的 message-only window 发停止消息，清理 listener 后销毁窗口。
- X11：XFixes 连接 fd 与私有取消 socket 一起阻塞等待。原有100ms轮询降级仍可被立即唤醒停止。
- Wayland：同样用私有取消 socket 唤醒协议事件循环。

`DEZIRCLIP_DISABLE_LINUX_SERVICES`（兼容 `TIEZ_DISABLE_LINUX_SERVICES`）的 `clipboard_listener` 或 `all` 可关闭监听；Wayland 后端另支持 `wayland_clipboard`。未启用捕获时不增加常驻 worker。

## 客户端与桌面降级

CLI、直接链接的 Core 客户端和 GUI 根据运行时会话选择同一原生后端。`capabilities --json` 与 GUI `get_platform_capabilities` 查询实际 registry/seat/data device；即使存在 DISPLAY，Wayland 会话也不会改用 X11 的剪贴板、来源身份或按键注入。只有一次性继承的 WAYLAND_SOCKET 且没有可重连 WAYLAND_DISPLAY 时，返回 `WaylandInheritedSocketUnavailable`，不会在探测时消耗该 fd 后宣称可用。

`runtime::EmbeddedClipboardWriter` 持有进程内 selection owner，供已有 Host/Service 注入 Core ClipboardService；`stop()` 成功确认其 owner 退出，后续 write 被拒绝。它不运行 current_exe 的隐藏子命令。独立 dzc 命令仍通过其明确的 clipboard-owner 子进程保留 selection；正文只经 stdin 传输，不进入 argv。默认 `write()` 保留监听入库；已有历史记录的复制显式使用 `write_suppressing_recapture()`，GUI 则传播既有 `mark_as_self_copy` 意图。只有该意图才加私有进程 MIME，避免普通历史复制重复；生成文本/OCR 不会被无条件过滤，仍经过 Core 隐私 ticket 和 epoch 检查。标记不能当隐私许可。新 owner 协商失败保留原 owner；显式停止不以 Drop 代替确认。

受保护的文本、HTML、图像与文件复制保留 Core 原始许可，在同一个 Wayland offer 中附带 KDE secret。许可不会编码为 MIME 正文；每次发送及每段 pipe 写入前重新检查数据库身份与隐私 epoch。隐私撤销同步停止并 join 本进程持有的私有 owner，未确认退出必须报错并保留 handle；不按名称寻找或终止其他进程。GUI 的受保护复制使用长期保留的进程内 writer，不把图形程序当作 dzc 子命令启动。服务显式持有自己的 writer，独立 CLI 继续使用其专用子进程。

受保护文件只导出 kernel-owned 文件句柄，owner 保留句柄至撤销、替换或停止。Wayland 私有 owner 与 X11 文件 owner 在每次供数前检查许可，另以100ms边界检查撤销/最多五分钟存活期；普通监听和普通 owner 不因此增加轮询。X11 文件使用受限的 URI/KDE secret/TARGETS owner，避免通用库 canonicalize memfd URI。X11 的文本、HTML 与图像仍使用原生库提供同 offer secret，由 Host 同步撤销其进程内 owner。已获得文件句柄的外部读取者可以完成现有读取，撤销阻止新的打开；不能宣称撤回其他应用已读取的字节。

捕获读取器在正文前读取同一 offer 的 KDE hint，交给 Core 隐私预检；明确 secret 直接跳过。Windows/X11 使用各自受限预检，X11 缺少足够 selection 跟踪证据时不会把空结果当作完整检查。GUI 历史项在 Wayland 中执行手动复制，保留当前搜索和记录，不自动粘贴或按“粘贴后删除”修改历史。

全局快捷键、自动粘贴、快速粘贴浮层、跨应用焦点恢复、全局定位/贴边、置顶、透明调整与截图目前没有获授权的原生适配器；GUI 禁用对应设置并说明原因，后端也在任何 X11/xdotool 操作之前拒绝。现有设置值不改写，重新在 Windows/X11 打开时仍可使用。Portal 探测只查询已运行服务的接口版本，不激活服务、不创建授权会话；接口存在不等于用户已授权，也不使上述能力变为 available。

能力查询不读取正文；GUI 每个 WebView 只发起一次查询，探测失败时交互保持禁用并提示重新打开窗口。`is_finished()` 只表示监听 worker 已结束；释放捕获所有权仍必须等 `stop()` join 确认。

## 当前验证与边界

P5-A 的后端测试在 Ubuntu24.04 WSL、Sway1.9 / wlroots0.17.1 的 headless pixman 合成器完成；使用清空环境的 bubblewrap、私有 user/PID/network/IPC namespace、HOME、runtime 和 display，禁用 XWayland，不接触宿主剪贴板。实际验证了 wlr 的文本/HTML、KDE secret marker、替换 owner、取消监听与 owner、selection 清空，以及统一订阅 ready/stop/join。Xvfb 私有 display 另验证 XFixes 和原轮询降级的订阅停止。

ext 分支已有类型/版本优先级回归，但 P5-A 尚未在提供 ext 的真实合成器执行；后续 P5-C 补充协议矩阵与断线负控。P5-A 已将 NativeCaptureReader 绑定同代 Wayland offer，并在 Core preflight 完整接线前防御拒绝明确的 secret 来源；P5-B 补齐 CLI/GUI 写入、完整能力报告与桌面交互降级。上述分阶段范围不表示已完成全局快捷键、自动粘贴、Surface 焦点恢复、置顶、透明或截图；PRIMARY 也未作支持承诺。
