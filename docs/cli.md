# dzc CLI 使用文档

`dzc` 是 DezirClip 的本地数据与剪贴板接口，支持 Windows 和 Linux X11。历史命令直接访问桌面端共用的 SQLite 数据库，GUI 未运行时仍可使用；原生系统剪贴板命令需要可访问的图形会话，OSC52 文本写入使用控制终端，可在 SSH 会话中工作。`dzc` 本身不发起网络请求。

记录 ID 是当前数据库内的整数标识，`timestamp` 是毫秒级 Unix 时间戳。`list`、`search` 按置顶顺序、时间和 ID 排序；`get latest` 按时间和 ID 取最新记录，不受置顶顺序影响。导入替换、删除或更换数据库后，应重新查询 ID。

TUI、fzf/dmenu/rofi 选择器、Shell 输入行插入和只读 MCP 内置于同一份 `dzc`，不需要 Python。**这些原生入口自 `v0.1.20-alpha.2` 起提供。** 使用前先查看 `dzc --help`，并选择包含相应命令的构建。原有 [Python 适配器](../integrations/README.md) 保留为兼容入口。Shell 插入将选中内容放入编辑缓冲区，供用户确认；复制到系统剪贴板与发送粘贴按键是不同操作。

## 安装与路径

`dzc` 随 DezirClip 作为 sidecar 安装，也可从 [GitHub Releases](https://github.com/qiyuanhuakai/dezirclip/releases) 下载独立 CLI 压缩包。选择匹配 Windows/Linux 与 x86_64/aarch64 的版本，解压后将目录加入 `PATH`，也可直接使用完整路径。桌面安装时可在设置中查看 CLI 路径。Linux 的安装位置随 deb、rpm、AppImage 而不同；AppImage 挂载路径不是永久安装目录。以下新增选项需要相应版本的 CLI，旧版发布包可能尚未包含。

Windows 裸 CLI 不依赖 WebView2。带 `COMPATIBILITY.json` 的新 CI 制品及后续独立发布包静态链接 MSVC 运行库，无需另装 Visual C++ 运行库。已发布的 alpha.1/alpha.2 旧压缩包保持原样，缺少 `VCRUNTIME140.dll` 时仍需安装匹配架构的 [Microsoft Visual C++ v14 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)。该变化不修改桌面安装包的运行要求。

自 `v0.1.21` 起，Linux 独立包在对应 x86_64 / aarch64 架构的 AlmaLinux 8 中编译、链接，面向 glibc 2.28+，并在 AlmaLinux 8、Ubuntu 22.04、Debian 12 的干净环境验证实际包；仍需要 D-Bus/GCC 运行库。这是 GNU/glibc 包，不提供 musl 或 Alpine 原生兼容。此门槛仅适用于独立 CLI：Linux 桌面包仍在 Ubuntu 24.04 构建，保留 X11 / WebKitGTK 4.1 等要求。旧 `v0.1.20` / `v0.1.20-alpha.3` Linux 独立附件仍要求 glibc 2.35+，alpha.1/alpha.2 仍要求 glibc 2.39+；旧资产不会被替换。详见 [独立 CLI 说明](./dzc-standalone.md)。

AppImage 运行时的临时挂载目录会随退出而失效，不要把其中的 `dzc` 永久软链接到 `PATH`。终端、Shell 插件或 MCP 需要稳定路径时，使用独立 CLI 压缩包并解压到固定目录。

Linux 普通安装的“加入 PATH”会在 `~/.local/bin/dzc` 创建链接；如果当前 Shell 的 PATH 不含 `~/.local/bin`，请自行加入该目录。程序不会修改 Shell 启动文件。设置页会提示 PATH 命中另一份 CLI 的情况，已有独立可执行文件或链接不会被覆盖。

Flatpak 的沙箱数据目录与宿主机独立 CLI 的默认目录不同，两者不会默认共享同一份历史。要访问 Flatpak 应用使用的环境和历史，可运行沙箱内附带的 CLI：

```bash
flatpak run --command=dzc io.github.qiyuanhuakai.dezirclip list --metadata --json
```

沙箱权限、图形会话和自定义目录设置仍然适用。不要把宿主机独立 CLI 的默认数据库误认为 Flatpak 数据库；检查各自数据路径，或在对应环境内明确指定 `DEZIRCLIP_DB_PATH`。

Shell 插件和 MCP 在宿主机运行时应使用独立 CLI 的稳定路径；沙箱内的 `/app/bin/dzc` 不能直接作为宿主机启动命令。

Windows 当前用户安装通常位于 `%LOCALAPPDATA%\DezirClip\dzc.exe`，以实际安装目录为准。当前 PowerShell 会话可临时加入路径：

```powershell
$env:Path += ";$env:LOCALAPPDATA\DezirClip"
dzc --version
dzc --help
```

应用和独立包附带 `skills/dzc-cli/`（CLI 工作流）及 `skills/dezirclip-theme/`（完整 CSS 主题构建）。工具设置提供各自的目录入口；复制整个 Skill 目录到 Agent 的 skills 目录即可使用。`dzc-cli` 中的安装脚本只链接技能文档，不安装 CLI 二进制。源码构建与 Linux 依赖见 [安装指南](./LINUX_INSTALL.md)。

## 命令速查

| 命令 | 用途 | 常用选项 |
| --- | --- | --- |
| `list [N]` | 列出历史，默认 20 条，`-1` 全部 | `--kind`、`--tag`、`--pinned`、`--offset`、`--json`、`--metadata`、`--tsv`、`--ids`、`--exclude-sensitive`、`-q` |
| `search <QUERY>` | 搜索历史，默认 20 条 | `--kind`、`--mode`、`--limit`、`--offset`、`--json`、`--metadata`、`--tsv`、`--ids`、`--exclude-sensitive`、`-q` |
| `get <ID\|latest>` | 读取完整存储内容 | `--preview`、`--raw`、`--mime`、`--json`、`--exclude-sensitive`、`-q` |
| `clipboard read` | 读取当前系统剪贴板 | `--mime`，默认 `text/plain` |
| `clipboard write [CONTENT]` | 写入原生剪贴板或向终端发送 OSC52，省略内容时读标准输入 | `--mime`、`--target native\|osc52`、`--osc52-passthrough none\|tmux`；内容支持文本、`-`、`@文件` |
| `clipboard formats` | 列出可读取的剪贴板 MIME 格式 | `--json` |
| `copy <ID>` | 恢复到原生剪贴板，或通过 OSC52 复制文本 | `--target native\|osc52`、`--osc52-passthrough none\|tmux`、`--quiet` |
| `pick` | 内置 TUI 选择，或调用可选的外部选择器 | `--backend fzf\|dmenu\|rofi`、`--query`、`--kind`、`--limit`、`--offset`、`--action`、`--osc52-passthrough` |
| `tui` | 终端浏览、预览、标签、置顶和删除 | `--query`、`--kind`、`--limit`、`--offset`、`--action`、`--osc52-passthrough` |
| `mcp` | 标准输入/输出上的只读 MCP 服务 | 无 |
| `theme validate <FILE>` | 校验完整 CSS 主题包，不打开历史数据库、不导入或应用 | `--json`；见[主题指南](./THEMES.md) |
| `bash` / `zsh` / `fish` / `pwsh` | 输出当前 CLI 对应的 Ctrl-G Shell 集成脚本 | 无；`powershell` 是 `pwsh` 别名 |
| `nu` | 输出 Nushell 集成，或写入指定文件 | `--install <PATH>`；`nushell` 是别名 |
| `add <CONTENT>` | 添加历史；内容支持文本、`-`、`@文件` | `--type` |
| `edit <ID> <CONTENT>` | 替换历史内容，重建预览 | `-q` |
| `delete <ID>` | 删除指定记录 | `--yes` 必填 |
| `clear` | 删除未置顶且无标签的记录 | `--yes` 必填，`-q` |
| `pin <ID>` / `unpin <ID>` | 置顶或取消置顶 | `-q` |
| `tag add/remove <ID> <TAG>` | 添加或移除标签 | 无 |
| `tag list <ID>` | 列出记录标签 | 无 |
| `count` | 统计记录总数 | `--json` |
| `stats` | 类型与使用次数统计 | `--top N`、`--json` |
| `export <PATH>` | 导出历史 | `--encrypted`、`--passphrase`、`-q` |
| `import <PATH>` | 导入历史 | `--mode merge\|replace`、`--passphrase`、`-q` |
| `watch` | 轮询数据库中的新增记录 | `--pattern`、`--interval`、`--json`、`-q` |

## 原生选择器与终端管理

本节命令自 `v0.1.20-alpha.2` 起提供。`pick` 不指定后端时打开内置 TUI；`tui` 直接打开同一界面。默认选中后恢复到原生系统剪贴板：

```bash
dzc pick
dzc tui --query project --kind text
dzc pick --backend fzf
dzc pick --backend dmenu --query project
dzc pick --backend rofi --action print
```

内置 TUI 不需要 fzf、Python 或桌面 GUI，需要交互控制终端；SSH 中可用 `ssh -t` 分配 PTY。外部后端必须已经安装并位于 `PATH`，Rofi 使用 `-dmenu` 协议。后端缺失会报错，不自动替换。外部程序只接收经过单行与控制字符清理的元数据；选中值必须精确对应候选项，条目正文不会作为命令执行。`--query` 为大小写不敏感的子串搜索，`--kind` 按存储类型筛选；默认每页 100 条，`--limit` 接受 1–200，`--offset` 为非负整数。

TUI 中方向键或 `j/k` 移动，`/` 输入搜索，`PageUp/PageDown` 或 `[/]` 翻页，左右键或 `h/l` 滚动预览，`r` 刷新，`c` 复制但不退出，`p` 切换置顶，`t/u` 添加/移除标签。`d` 删除时需要输入 `yes`；`Enter` 确认选中，`Esc`、`q` 或 `Ctrl-C` 取消。取消返回非零退出码，不输出选中内容。TUI 的预览清理控制字符；正文输出保留存储内容。

`pick` 与 `tui` 共用以下选中动作：

| `--action` | 行为 |
| --- | --- |
| `copy`（默认） | 恢复文本、富文本、图片或文件到原生系统剪贴板 |
| `print` | 将文本正文原样写到标准输出，不补换行 |
| `id` | 输出所选 ID 和一个换行 |
| `insert` | 输出供 Shell 插入的文本，不自动提交；拒绝 NUL 字节 |
| `osc52` | 向控制终端发送文本，适合 SSH 本地剪贴板；限制见下方 OSC52 小节 |

`print`、`insert`、`osc52` 仅接受文本、代码、URL、HTML 和富文本记录；图片、文件和视频请使用 `copy`。`--osc52-passthrough tmux` 只能配合 `--action osc52`。TUI 本身是本地历史管理入口，可能显示敏感条目；Agent 使用下方默认过滤敏感记录的 MCP 服务。

```bash
dzc tui --action print > selected.txt
dzc pick --backend fzf --action id
dzc tui --action osc52
dzc pick --backend fzf --action osc52 --osc52-passthrough tmux
```

## Shell：Ctrl-G 插入当前命令行

以下原生入口自 `v0.1.20-alpha.2` 起提供，只依赖 `dzc` 和所用 Shell。将对应初始化行加入自己的 Shell 启动文件可持久启用，也可先在交互会话中运行：

```bash
# Bash 5+
eval "$(dzc bash)"
# Zsh
eval "$(dzc zsh)"
# Fish
dzc fish | source
```

```powershell
# PowerShell 7，需加载 PSReadLine
dzc pwsh | Out-String | Invoke-Expression
```

`Ctrl-G` 默认打开内置 TUI，选中后在光标处插入正文，等待用户确认，不按 Enter。取消或选择失败时保留原输入。初始化脚本绑定生成它的 CLI 完整路径，不会执行剪贴板内容；上面的 `eval` / `Invoke-Expression` 只加载 `dzc` 生成的 Shell 定义。`DZC_BIN` 可覆盖 CLI 路径，`DZC_PICKER` 可选择 `tui`（默认）、`fzf`、`dmenu` 或 `rofi`。例如在 Bash/Zsh 中先设置 `export DZC_PICKER=fzf`，PowerShell 中设置 `$env:DZC_PICKER = 'fzf'`；外部后端仍需自行安装。

Nushell 需要 **0.116+**。Nushell 不支持从运行时生成的字符串直接 `source`，可显式安装到用户自动加载目录，然后重启 Nushell：

```nu
let widget = ($nu.user-autoload-dirs.0 | path join 'dzc.nu')
dzc nu --install $widget
```

也可以 `dzc nu --install /absolute/path/dzc.nu` 后在自己的 `config.nu` 中加入 `source /absolute/path/dzc.nu`，通过 `config nu` 编辑。Windows 使用引号和正斜杠，例如 `source 'C:/Tools/dzc/dzc.nu'`。`--install` 只写入指定的 `.nu` 文件并按需创建父目录，重复安装相同内容可安全执行；已有不同内容或符号链接会报错，不会改写配置。Nushell 的 `Ctrl-G` 适用于 emacs、vi_insert 和 vi_normal；可在加载前设置 `$env.DZC_BIN` 或 `$env.DZC_PICKER`。

这些命令只修改当前会话或显式指定的 Nushell 文件，不自动编辑其他 Shell 启动文件、终端或 tmux 设置。

## 原生只读 MCP

自 `v0.1.20-alpha.2` 起提供的原生服务直接使用共享 Rust 数据访问层，不需要 Python、MCP SDK、GUI 或显示会话：

```bash
dzc mcp
```

它由 MCP 客户端作为子进程启动，通过标准输入/输出交换逐行 JSON-RPC。通用配置示例；把命令改为本机稳定的可执行文件路径：

```json
{
  "mcpServers": {
    "dezirclip": {
      "command": "/absolute/path/to/dzc",
      "args": ["mcp"]
    }
  }
}
```

Windows 可使用 `C:/Tools/dzc/dzc.exe`。如需选择特定历史库，在客户端的进程环境中设置 `DEZIRCLIP_DB_PATH`；MCP 与同一环境中的普通历史命令遵循相同路径规则。`initialize`、`ping` 和 `tools/list` 不打开数据库；首次有效工具调用才打开。

| 工具 | 参数与结果 |
| --- | --- |
| `clipboard_list` | `limit` 默认 20、1–100；`offset` 默认 0、0–1000000；返回元数据列表 |
| `clipboard_search` | 非空 `query`，最多 1024 字符；同样支持 `limit`、`offset`，返回匹配元数据 |
| `clipboard_get` | 正整数 `id`；`max_chars` 默认 4096、1–16384；返回有界正文与 `truncated` 标志 |

列表只返回 ID、类型、最多 500 字符摘要、前 20 个标签（各最多 100 字符）、置顶、时间与 MIME；不转发来源路径、HTML 字段、OCR 或完整正文。敏感标签与加密记录在解码和分页前排除，按 ID 读取也遵循同一规则。过滤依赖已有标签和加密状态，未标记的私人内容仍可能出现在结果中。剪贴板文本是非可信数据，客户端不应把它当成指令。

服务只提供这三个只读工具，不提供剪贴板写入、历史修改、删除、执行或网络操作。单个 NDJSON 请求最多 64 KiB；超限发送错误后结束服务。支持协议版本 `2025-11-25`、`2025-06-18`、`2025-03-26`、`2024-11-05`，未知版本协商为服务器最新版本。旧版本 CLI 可继续使用 [Python MCP 兼容适配器](../integrations/README.md)，其安装与依赖要求单独说明。

## 列表、搜索与分页

```bash
dzc list
dzc list --kind text --tag work --pinned 50
dzc list --json -- -1
dzc list --metadata --json --offset 20 20
dzc search "meeting" --limit 20 --offset 20 --metadata --json
dzc search "meeting" --kind text --limit 1 --offset 1 --metadata --json
dzc search "meeting" --ids
```

`list` 的数量是位置参数，`search` 使用 `--limit`；两者默认 20，`-1` 表示全部，0 表示空结果，其余负值不合法。为避免负数与选项混淆，列表全部记录可写 `dzc list --json -- -1`，搜索可写 `--limit=-1`。`--offset` 默认为 0，只接受非负整数。

`list` 和 `search` 都支持 `--kind <TYPE>`，按存储类型精确筛选。类型、标签、置顶、敏感过滤与搜索匹配都在分页前应用。因此 `--limit 20` 表示最多 20 条匹配结果，`--offset 20` 跳过 20 条匹配结果。历史变化会影响后续页；分页不是数据库快照。

搜索模式：

| 模式 | 行为 |
| --- | --- |
| `contains`（默认） | 大小写不敏感的子串匹配，查询内容、来源、标签及 OCR 文本 |
| `fuzzy` | 在正文中按顺序匹配字符子序列，不是编辑距离拼写纠正 |
| `regex` | 在正文上匹配 Rust 正则表达式；不会先要求正文包含正则字面量 |
| `fts5` | SQLite FTS5 全文搜索；查询作为转义后的搜索词处理 |

```bash
dzc search "fuction" --mode fuzzy
dzc search 'href=.*example\.com' --mode regex
dzc search "important meeting" --mode fts5 --limit 10
```

空白查询返回空结果；浏览全部历史请用 `list`。非法正则报错。没有 `exact` 模式。

### JSON、元数据和 TSV

`list --json`、`search --json` 输出数组；`get --json` 输出单个对象。完整 JSON 保留存储的 `content`，不再使用 GUI 列表的 2000 字符内容截断：

```json
{
  "id": 142,
  "type": "text",
  "content": "  Hello\r\n",
  "preview": "Hello",
  "tags": ["work"],
  "pinned": false,
  "timestamp": 1768461780000,
  "mime": "text/plain",
  "sensitive": false
}
```

`list/search --metadata --json` 保留 ID、类型、最多 500 字符的摘要、标签、置顶、时间、`mime` 和 `sensitive`，省略 `content`，适合选择器先列摘要、选中后再读取正文。元数据摘要仍可能含私人信息；`--metadata` 不等于隐私过滤。

`--ids` 每行输出一个 ID，列表和搜索均支持。`--tsv` 每行输出 `ID<TAB>preview`，将多行空白压成单行并移除控制字符，适合 fzf/dmenu 输入：

```bash
dzc list --metadata --tsv --exclude-sensitive 100
dzc search "project" --ids
```

`--json`、`--ids`、`--tsv` 用于不同输出模式，请每次选择一种。默认人类可读输出为带类型图标的单行摘要。`--quiet` 只保留错误与退出码；`watch --quiet` 例外，仅隐藏启动横幅，仍输出新增记录。

### 排除敏感记录

```bash
dzc list --metadata --json --exclude-sensitive
dzc search "project" --json --exclude-sensitive
dzc get latest --raw --exclude-sensitive
```

`--exclude-sensitive` 在解密和结果解码前排除敏感标签（包括 `sensitive`、`密码`）与已加密记录，并在过滤后分页。`get latest --exclude-sensitive` 返回最新的可见记录；指定 ID 被排除时与不存在的 ID 一样报错。敏感检测依赖已有标签和加密状态，不能识别所有尚未标记的私人内容。

桌面端另有来源应用排除设置 `app.excluded_source_apps`，默认空字符串。设置界面每行输入一个应用名、可执行文件名或完整路径，忽略大小写，完整匹配，可省略 `.exe`，离开输入框后应用。它在自动捕获存储前拦截后续内容，不删除既有记录；手动 `dzc add` 仍按明确请求添加。X11 优先识别剪贴板所有者的 PID，缺少时回退前台应用，无法识别的来源不能可靠排除。

## get：原始内容与 MIME

```bash
dzc get 142
dzc get latest --json
dzc get 142 --preview
dzc get 142 --raw > original.txt
dzc get 142 --mime text/html > fragment.html
dzc get 143 --mime image/png > image.png
dzc get 144 --mime text/uri-list > files.txt
```

默认输出完整存储内容并补一个换行；`--raw` 原样输出存储内容的 UTF-8 字节，不补换行。原始首尾空格、制表符和 CRLF/LF 保留，预览与搜索规范化不会修改正文。此保证适用于新捕获及新增的数据；旧版本已经修剪掉的字符无法恢复。`--preview` 输出摘要；`--json` 输出结构化对象。

`--mime` 选择一种表示，输出该表示的原始字节，不补换行：

| MIME | 表示 |
| --- | --- |
| `text/plain` | 文本正文 |
| `text/html` | 保存的 HTML 富文本 |
| `image/png` | 解码图片后编码为 PNG 字节 |
| `text/uri-list` | 文件列表，使用转义后的 `file://` URI 和 CRLF 分隔 |

无对应表示时报错。图片记录的 `--raw` 是存储的数据 URL 或路径；获取图片文件应使用 `--mime image/png`。MIME 表示转换不承诺保留原始图片编码字节。重定向二进制内容时，应使用能保留原始字节的管道，避免 shell 的文本重编码。

## 系统剪贴板读写与历史恢复

```bash
dzc clipboard read
dzc clipboard formats
dzc clipboard formats --json
dzc clipboard read --mime text/html > current.html
dzc clipboard read --mime image/png > current.png
dzc clipboard write "Hello, World!"
printf '  first\r\nsecond\r\n' | dzc clipboard write -
dzc clipboard write @fragment.html --mime text/html
dzc clipboard write @screenshot.png --mime image/png
dzc clipboard write @files.txt --mime text/uri-list
dzc copy 142 --quiet
```

`read` 与 `write` 的 `--mime` 默认都是 `text/plain`，可选格式与上表一致；`read` 不添加换行，`formats` 每行输出一个受支持、当前可读取的 MIME，`--json` 改为 JSON 数组。它不是操作系统全部私有格式的枚举。

`write` 的内容可来自字面文本、`-` 标准输入或 `@<路径>` 文件。图片应通过文件或二进制标准输入提供。`copy ID` 恢复历史记录的文本、HTML、图片或文件格式；它只更新剪贴板，之后由用户或 Shell 集成决定如何使用，不自动按 Enter 或向目标应用发送粘贴快捷键。`get`/`add` 操作历史，`clipboard read/write` 操作当前剪贴板；GUI 正在监听时，系统剪贴板写入可能被正常捕获为历史。

Windows 使用原生剪贴板接口。Linux X11 不依赖 `xclip`、`xsel` 或 `wl-copy`：写入后由轻量后台进程持有 selection 并响应粘贴请求，其他应用替换剪贴板后自动退出。GUI 关闭时也能写入。后台进程被手动终止或 X11 会话结束后，内容可能失效。GUI 自动粘贴按键仍使用 `xdotool`。

无可访问的 X11/Windows 桌面会话时，历史命令仍可工作，原生系统剪贴板命令会报错。SSH 下 `--target native` 操作的是进程能访问的远程图形会话，不自动转发到本地电脑。完整 Wayland 桌面支持暂缓；通过终端发送 OSC52 文本不等于支持 Wayland 后台剪贴板监听。

### SSH：用 OSC52 将远程文本复制到本地

```bash
# 在本地连接远程主机；远程已安装相应版本的 dzc
ssh -t user@server

# 以下命令在 SSH 会话中运行
dzc copy 142 --target osc52
printf 'remote note\n' | dzc clipboard write - --target osc52
```

`--target` 默认为 `native`，不会自动回退到 OSC52。显式使用 `osc52` 时，`dzc` 将文本编码为 [OSC52 剪贴板写入序列](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)，直接写入控制终端（Unix 的 `/dev/tty`，Windows 的 `CONOUT$`），不把控制序列混入标准输出，也不需要远程 `DISPLAY` 或桌面 GUI。SSH 将终端输出传回本地终端，由它决定是否更新本地剪贴板。

`clipboard write --target osc52` 仅接受 `text/plain`。`copy --target osc52` 接受文本、代码、URL 和富文本的文本正文，拒绝图片、文件列表和视频；HTML 按记录中保存的文本内容发送，不在终端渲染格式。内容最多 **100000 个原始 UTF-8 字节**，超出时报错而不截断；终端自身的大小限制可能更低。没有控制终端时也会报错。

本地终端必须支持并允许 OSC52，可能弹出权限提示或直接拒绝。成功退出仅表示已发送序列，无法确认本地剪贴板已更新；`dzc` 不执行 OSC52 读取或查询。远程 `dzc list/search` 访问远程历史，OSC52 不同步历史，也不会读取本地剪贴板。

如果当前命令运行在 tmux 内，且需要显式穿透到外层终端，可使用：

```bash
dzc copy 142 --target osc52 --osc52-passthrough tmux
```

默认穿透模式为 `none`，`tmux` 只包装一层；配合 `native` 目标使用会报错。tmux 3.3+ 需要用户自行允许 passthrough，例如在自己的 `~/.tmux.conf` 中加入 `set -g allow-passthrough on` 并重新加载；仅在确实使用该穿透模式时配置。DezirClip 不修改 tmux 或终端设置。细节见 [tmux 官方说明](https://github.com/tmux/tmux/wiki/FAQ#what-is-the-passthrough-escape-sequence-and-how-do-i-use-it)。

## 添加与编辑历史

```bash
dzc add "Hello, World!"
printf '  exact\r\n' | dzc add - --type text
dzc add @report.txt --type text
dzc add "<p>你好，<b>世界</b></p>" --type html
dzc add "fn main() {}" --type code
dzc add @screenshot.png --type image
dzc edit 142 @replacement.txt --quiet
```

`add` 默认类型为 `text`，成功输出 `Added: id=<ID>`，拒绝空内容。`@文件` 读取文件内容；`--type image` 支持读取图片文件或二进制标准输入，验证后保存为图片数据 URL。添加文件路径记录可用 `dzc add "/path/to/file" --type file`，它与把文件正文加入历史不同。

`edit` 的内容源同样支持文本、`-` 和 `@文件`；替换 `content` 并以其前 500 个字符重建摘要，保留标签与置顶状态。编辑 `rich_text` 时清除旧 HTML 并转为纯文本，避免恢复已过时的格式；其他类型保持原类型。成功输出 `Edited: id=<ID>`，可用 `--quiet` 静默。`add` 不接受 `--tag`、`--pin`，请随后调用标签和置顶命令。

## 删除、清理、置顶和标签

```bash
dzc delete 142 --yes
dzc clear --yes
dzc pin 142
dzc unpin 142 --quiet
dzc list --pinned
dzc tag add 142 work
dzc tag remove 142 work
dzc tag list 142
```

`delete` 和 `clear` 必须传 `--yes`，缺少时直接报错，不弹确认框。删除不可恢复。`clear` 仅删除未置顶且没有任何标签的记录，成功输出 `Cleared <N> entries`，支持 `--quiet`。`delete` 成功无输出。

`pin`、`unpin` 支持 `--quiet`；ID 不存在时报错。重复添加同一标签、移除不存在的标签都可安全重复执行。无标签时 `tag list` 输出 `(no tags)`。

## 统计

```bash
dzc count
dzc count --json
dzc stats --top 10
dzc stats --json
```

`count --json` 输出 `{"count":1234}`。`stats` 包含总数、按类型计数和最常用记录；`--top` 默认为 5，0 隐藏最常用记录区块，JSON 字段为 `total`、`by_type` 和 `most_used`。

## 导出、导入和手动迁移

```bash
dzc export /tmp/backup.json
dzc export /tmp/backup.dzc --encrypted --passphrase "my-strong-pass"
dzc import /tmp/backup.json --mode merge
dzc import /tmp/backup.dzc --mode merge --passphrase "my-strong-pass"
```

`export` 支持明文 JSON 与加密备份，导出完整正文，不应用列表内容截断。`.dzc`/`.tiez` 扩展名也会选择加密。加密需要至少 12 个字符的 `--passphrase`，缺少时直接报错。

图片记录会将原始图片字节嵌入备份，保留已有数据 URL，迁移后无需原附件路径。图片附件丢失或无法解码时，导出明确报错，并保留已有目标备份。文件列表和视频记录仅保存原文件引用，不打包文件本身；HTML 正文保留，但其中链接的本地图片、样式等资源不会自动嵌入。

桌面端与 CLI 使用相同的完整备份和恢复逻辑。备份通过同目录临时文件完整写入后替换目标，不能覆盖当前数据库或其日志文件。具有有效历史密文结构但无法成功解密的字段会使导出报错；请恢复原加密密钥的访问权限后再备份。普通 `linux:` / `dpapi:` 前缀文本会原样导出；历史格式对完全仿造密文结构的文本存在歧义，详见 [数据安全说明](./data-safety.md#备份能保留什么)。

`import --mode merge` 保留备份中的 ID，以导入记录覆盖同 ID 记录，其余记录保留；稀疏或乱序的 ID 也保持不变。`--mode replace` 删除全部现有历史，包括置顶和带标签的记录，再导入，应在确认并备份后使用。导入在单次数据库事务中完成，失败时回滚。两者均支持 `--quiet`。

恢复前会验证全部图片；无效图片、非正整数或重复 ID 均会中止操作。正文、HTML、图片原始字节、标签、置顶顺序、使用次数、内容分类及 OCR 文本会保留。恢复失败不会删除既有历史或改写原附件。数据库升级同样按事务执行；较旧程序遇到较新数据库会拒绝打开，请安装匹配的 GUI 与 CLI 版本。GUI 与 CLI 同时访问共享库时，缓存会在其他连接提交后刷新。

升级或磁盘访问失败会保留原数据库并返回错误，不会自动换成空历史。请先排查权限、剩余空间及错误信息，再重试或使用已有备份恢复。

迁移到另一台机器时，手动传输导出文件，再执行导入。DezirClip 不包含云同步或自动跨设备同步功能，也没有对应开发承诺。

## watch：新增历史事件流

```bash
dzc watch --json
dzc watch --quiet --interval 1000
dzc watch --pattern "urgent" --json
```

`watch` 轮询数据库中的新增记录，不直接监听操作系统剪贴板；桌面端关闭且没有其他写入者时，不会产生自动捕获的新记录。默认轮询间隔 500 ms，`--interval` 限制在 50–60000 ms。`--json` 为逐行 NDJSON，`--quiet` 仅隐藏启动横幅。Ctrl-C 退出码为 130。

`--pattern` 是大小写不敏感的子串，不是正则：`password|secret` 只匹配这个完整字面量。`watch` 没有 `--notify`、`--cmd`、`--exclude-sensitive`，不应把未经筛选的事件流直接转发给外部服务。Agent 查询使用上方带隐私过滤的原生 `dzc mcp`，旧版本可使用 [Python MCP 兼容适配器](../integrations/README.md)。

## 自定义 fzf / dmenu 管道

原生 `dzc pick --backend fzf` / `--backend dmenu` 已处理候选显示、选中校验和输出动作。需要组合自己的管道或兼容缺少原生命令的版本时，也可直接使用稳定的 TSV 接口：

```bash
id=$(dzc list --metadata --tsv --exclude-sensitive 100 |
  fzf --delimiter="$(printf '\t')" --with-nth=2.. | cut -f1)
if [ -n "$id" ]; then
  dzc copy "$id" --quiet
fi
```

此示例在取消或无结果时不执行复制，展示内容由 `--tsv` 清理为安全单行。原生入口见上方选择器和 Shell 小节；旧版 Python 选择器与 Rofi script mode 见 [integrations](../integrations/README.md)。不要用 `eval` 执行选中的剪贴板内容。

## 数据库路径与退出码

| 环境变量 | 说明 |
| --- | --- |
| `DEZIRCLIP_DB_PATH` | 指定 SQLite 文件路径 |
| `TIEZ_DB_PATH` | 旧别名，仅上一项未设置时生效 |

默认路径：

- Windows：`%APPDATA%\io.github.qiyuanhuakai.dezirclip\clipboard.db`，不存在时回退旧目录 `%APPDATA%\com.tiez.clipboard\clipboard.db`。
- Linux：`$XDG_DATA_HOME/io.github.qiyuanhuakai.dezirclip/clipboard.db`，未设置时使用 `$HOME/.local/share/io.github.qiyuanhuakai.dezirclip/clipboard.db`。

这些变量选择历史数据库，不改变系统剪贴板会话。Linux 原生剪贴板还依赖 `DISPLAY`、X11 授权等会话环境。

未指定数据库环境变量时，CLI 与 GUI 使用相同的数据目录规则：可执行文件旁已存在的 `data/` 目录优先；否则遵循默认应用数据目录中的 `datapath.txt` 自定义目录指向，再回退到默认目录。处于同一宿主机用户环境的普通独立 CLI 与桌面安装共享历史；Flatpak 沙箱的默认环境单独隔离，见上方安装说明。单独创建 `data/` 会让该解压位置使用独立的可携式历史。GUI 首次启动不会用旧目录的较大数据库覆盖已由 CLI 创建的新数据库。

| 退出码 | 含义 |
| --- | --- |
| `0` | 成功 |
| `1` | 操作失败，或取消选择/无选中项；例如记录不存在、缺少 `--yes`、数据库/剪贴板/I/O/解密错误 |
| `2` | 参数解析错误，如未知选项、非法模式或负偏移 |
| `130` | 用户中断 `watch` |

若提示 `database is locked`，等待另一个写入者完成再重试；无需启动 GUI。搜索无结果时检查模式、过滤条件及目标记录的正文或 OCR 文本。导入格式错误时确认文件由 `dzc export` 生成，加密备份需使用正确密码。
