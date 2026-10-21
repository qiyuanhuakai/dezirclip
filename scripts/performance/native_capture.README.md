# Linux Desktop 原生捕获验收与采样

`native_capture.py` 测量真实 Linux Desktop 从外部进程写入系统剪贴板，到 GUI 收到实际持久化捕获事件的路径。旧版监听 `clipboard-updated`，候选版监听 `clipboard-captured`；候选事件必须只有 `id`。工具没有修改产品或生成替代事件。

时钟起点是实际 WebView 中的 `performance.now()`，终点是该 WebView 的原生事件回调。因此结果包含 WebDriver 启用计时后的返回、外部 CLI 启动、OS 剪贴板写入、监听、业务处理、SQLite 提交和事件传输。它不是纯 Core/Engine 耗时，也不是从 writer 完成后才开始计时。`writer_elapsed_ms` 单独保留。WebKit 的时钟精度保持原样，不对样本插值。

## 隔离与输入

需要 Linux、Python 3.13 或更新版本（仅标准库）、兼容 SQLite 3.50.4 或更新版本、bubblewrap、Xvfb、DBus 及匹配 WebKitGTK 的 WebKitWebDriver。Python 版本不代表 SQLite 版本，必须核对实际链接引擎；旧 3.45 的合成混写已出现完整性失败。生产 Core 自带固定的 SQLite 3.54.0。测试使用裸 Desktop release 二进制，不传 AppImage；工具不执行构建、不访问更新网络或签名密钥。

默认使用隔离环境中的 `/usr/bin/python3`。系统解释器不满足要求时，提供 `--python-runtime /readonly/python`，指向独立、完整且可重定位的运行时前缀（含真实 `bin/python3.13` 和标准库）。工具只将该前缀按原路径只读挂载，不挂载宿主用户目录；显式设置 Python home、禁用用户 site 和字节码写入。宿主预检、隔离环境和结束核对使用实际解释器、SQLite 版本/source ID 及关键文件哈希；内置 sqlite 模块由承载它的解释器/libpython 文件核验。可信合成工具的兼容检查不代替生产引擎的敏感词项清除验证。

每个块创建新的 user/PID/network/IPC/UTS/mount namespace、Xvfb、DBus、HOME 和 XDG profile。`--clearenv` 后只建立显式环境；宿主 display、session bus、用户目录、数据库和剪贴板不可见。剪贴板 owner 进程与 GUI 均在私有 PID namespace 内。除只读系统运行库、明确提供的二进制/工具/载荷外，只挂载本块的临时目录。临时根只能由外层工具创建为 `/tmp/dezirclip-native-capture-*`，内层核对普通 marker 文件及 namespace。证据不会自动删除。

GUI 使用标准 `$XDG_DATA_HOME/io.github.qiyuanhuakai.dezirclip/clipboard.db`，启动后通过真实 `get_data_path` 核对。每端先使用其对应版本 CLI 初始化 schema，再通过 `sqlite_history_fixture` 测试连接插入相同中性历史。旧版保持原 schema，当前候选使用 schema 21，不让旧程序写新库；旧冻结树的报告保留其实际 schema，不重贴标签。persistent 与 deduplicate 开启；persistent limit、idle destroy、privacy detection、OCR、sequential 和 sound 关闭；rich text、files 开启。实际设置、schema 及环境随报告保存。只关闭 window tracker、edge docking、mouse hotkey 服务，剪贴板监听保持启用。

两份 `build.json` 至少包含如下信息，完整原文件会复制保留：

```json
{
  "source_revision": "40位完整Git提交",
  "profile": "release",
  "target": "x86_64-unknown-linux-gnu",
  "rustc_version": "rustc 1.91.1 的完整输出，两端完全一致",
  "binaries": {
    "gui": {"sha256": "实际GUI哈希"},
    "cli": {"sha256": "实际CLI哈希"}
  }
}
```

默认旧源仍为 `802808b06e6b795064c5065d1d880bae9b193b9a`，监听原有 `clipboard-updated`。alpha.2 与已发布 alpha.1 比较时，须显式传入 `--baseline-revision 90ba9960744b6cd57caaaf597f68533a7e41be45`，并提供这个实际提交的独立 release 构建凭据；此时基线和候选均监听 `clipboard-captured`，两端都严格要求事件只有 ID。基线参数只接受这两个完整冻结提交，不能传分支、短 SHA 或任意选择较慢版本。原 alpha.1 工具快照和所有历史报告保持原样，不重新标注版本。

凭据还应保留归档核对、允许的构建隔离改动、前端锁文件与构建、构建命令及源码文件哈希。工具验证实际二进制与凭据一致，并拒绝两端 profile、Rust 1.91.1 或 target 不一致。正式两端使用同一个候选 CLI 执行 `clipboard write - --mime ...`；写入命令不初始化或迁移历史数据库。

## 先做功能检查

仅验证 namespace、环境与私有 Xvfb/DBus，不启动 GUI 或写剪贴板：

```sh
python3 scripts/performance/native_capture.py --mode isolation --python-runtime /readonly/python
```

低次数功能检查（两端各 1 个测量样本、1 个预热，1k/10k 历史、全部四场景）：

```sh
python3 scripts/performance/native_capture.py --mode functional --python-runtime /readonly/python \
  --old-gui /readonly/old/bin/dezirclip --old-cli /readonly/old/bin/dzc \
  --old-proof /readonly/old/build.json \
  --candidate-gui /readonly/candidate/bin/dezirclip \
  --writer /readonly/candidate/bin/dzc --candidate-proof /readonly/candidate/build.json \
  --webkit-driver /readonly/tools/WebKitWebDriver --samples 1 --warmup 1
```

可选 `--rows 1000` 或 `--cases text-200b` 缩小工具调试范围。功能模式最多 5 个样本、2 个预热，始终 `timing_eligible: false`。候选尚未冻结时可用 `--baseline-only`，省略候选 GUI/proof，并显式以旧 CLI 作为 `--writer`，只做旧 GUI 的工具冒烟；报告会标明使用旧 writer，禁止正式比较。

## 冻结后正式 ABBA

仅在源码、构建产物冻结及本任务静默窗口确认后运行。将冻结提交、产物哈希、本任务已停止的构建/其他采样、同机其余负载的控制范围和时段记录为普通文件，传入 `--quiet-window-note`。无法控制其他会话时应如实注明，不声称全宿主静默。该记录由操作者确认，工具不能自行证明机器没有其他负载；负载记录与完整 ABBA 轮次均需一起评估。

```sh
python3 scripts/performance/native_capture.py --mode formal --python-runtime /readonly/python \
  --old-gui /readonly/old/bin/dezirclip --old-cli /readonly/old/bin/dzc \
  --old-proof /readonly/old/build.json \
  --candidate-gui /readonly/candidate/bin/dezirclip \
  --writer /readonly/candidate/bin/dzc --candidate-proof /readonly/candidate/build.json \
  --webkit-driver /readonly/tools/WebKitWebDriver \
  --quiet-window-note /readonly/frozen-quiet-window.txt
```

每种历史规模按旧 A1 → 新 B1 → 新 B2 → 旧 A2 顺序，以独立新 profile 执行全部场景；每块每场景至少 5 个预热和 30 个测量样本。支持增加样本数，不允许省略任何场景/规模/轮次。全部非预热样本参与 p95，阈值为 `candidate <= baseline + max(baseline × 10%, 20ms)`。同时输出 p50 和 writer p95。保留各轮原始值，不挑选更快基线或丢弃失败轮次。

四场景是 200 字节与 1 MiB UTF-8/CRLF 文本、3 KiB HTML、3840×2160 RGBA 图像。图像通过有效 PNG 写给 CLI，经实际解码形成 33,177,600 字节 RGBA；报告分别记录编码字节数与解码尺寸，不把 PNG stdin 当作原始 RGBA stdin。固定 seed 生成同一载荷组供所有块使用，每个样本内容唯一。HTML 的 text fallback 与 HTML 均由真实 CLI 写入。每个完成样本后固定冷却 700 ms，并记录相邻 arm 的实际间隔，跨过 500 ms 通知合并窗口；不是精确的 700 ms 起点间隔。

每次事件必须对应 SQLite 中新增的预期 ID 且恰好新增一行。文本完整内容与 HTML 逐字核对；图像必须外置到当前 profile 的真实附件路径，核对 PNG CRC、3840×2160 RGBA 及唯一角点颜色。最终核对总行数与 SQLite `quick_check`。这些检查不依赖 UI 显示数量。候选事件的非 ID 字段会导致失败。

## 证据与失败

外层打印证据目录，包含总 `report.json`、完整 build proofs、quiet note、`tool-sources/` 中实际执行的工具快照、载荷及哈希、计划、各块 `block.json`、bootstrap/WebDriver/sandbox 日志、成功块窗口截图及实际 profile/附件。样本在 arm 前写入 `running` 记录，writer 返回码/输出/耗时和成功后置条件逐条保存。超时会终止整个块的进程组，保留已有样本与日志；其余既定 ABBA 块仍继续，最终失败退出非零。失败报告不会标为 timing eligible，原始样本仍可用于诊断。

二进制、WebKitWebDriver、工具源码、计划、proof 与载荷运行后再次核对哈希。报告记录 kernel、CPU、Python、两端编译器/profile/target、每块环境与开始/结束负载。sandbox 不同 namespace 与 DB path 断言失败时不能继续采样。此工具覆盖 Linux X11 真实 Desktop 完整路径；不证明 Windows、Wayland、安装器或更新签名行为，也不能替代普通/敏感/去重等 Core 独立性能报告。
