# DezirClip

Windows 和 Linux X11 上的本地剪贴板工作台：保存、搜索、恢复剪贴板历史，也能通过 `dzc` 把同一份数据接入终端、启动器和 Agent。

本项目派生自 [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard)，保留本地桌面能力，移除 macOS 与云同步等网络功能，并加入 Linux X11 支持、自定义 CSS 主题和 WebView 内存管理。当前迭代重点是内容保真、捕获可靠性和 CLI 生态。

[下载发布版本](https://github.com/qiyuanhuakai/dezirclip/releases) · [CLI 文档](./docs/cli.md) · [终端与 MCP 集成](./integrations/README.md) · [Linux 安装指南](./docs/LINUX_INSTALL.md) · [English](#english)

## 安装

需要图形界面时，从 Releases 下载 Windows 安装包或 Linux deb/AppImage/rpm。仅使用脚本、SSH 或终端工具时，可下载独立 `dzc`，不必运行桌面主程序。

| 独立 CLI 压缩包 | 平台 |
| --- | --- |
| `dzc-VERSION-windows-x86_64.zip` | Windows x64 |
| `dzc-VERSION-windows-aarch64.zip` | Windows ARM64 |
| `dzc-VERSION-linux-x86_64.tar.gz` | Linux x64 |
| `dzc-VERSION-linux-aarch64.tar.gz` | Linux ARM64 |

`VERSION` 对应发布版本号。每个独立压缩包包含真正的 `dzc` 二进制、许可证、说明、完整的集成运行文件目录 `integrations/` 和 `skills/dzc-cli/`，不含开发测试文件；对应 `.sha256` 文件用于校验下载。Linux 官方 CLI 构建基于 Ubuntu 24.04，要求 glibc 2.39+ 及其动态库；不宣称静态链接或兼容更早发行版。桌面端使用原生图形依赖，详见 Linux 指南。

下方命令描述当前分支的能力。旧版发布包可能缺少 `--metadata`、`--target osc52` 等选项，请使用匹配版本的 CLI 与适配器，并用 `dzc --help` 检查。

解压后将二进制目录加入 `PATH`；也可在适配器中用 `DZC_BIN` 指定完整路径。例如：

```powershell
# Windows，按实际解压目录修改；仅影响当前会话
$env:Path = "C:\Tools\dzc;$env:Path"
$env:DZC_BIN = 'C:\Tools\dzc\dzc.exe'
dzc --version
```

```bash
# Linux，假设解压到 ~/tools/dzc
export PATH="$HOME/tools/dzc:$PATH"
export DZC_BIN="$HOME/tools/dzc/dzc"
dzc --version
```

桌面安装也附带 `dzc`，可在设置中查看实际路径。同一宿主机用户环境中的普通独立 CLI 默认共享历史；Flatpak 沙箱默认目录独立，AppImage 挂载路径也不适合永久加入 `PATH`。具体用法见 [CLI 安装说明](./docs/cli.md#安装与路径)。更换机器不会自动获得另一台机器的历史，路径覆盖与可携式安装的区别见 [数据库路径说明](./docs/cli.md#数据库路径与退出码)。

## 桌面端能做什么

- 本地 SQLite 历史，支持文本、富文本、图片、文件、代码、URL、标签和置顶。
- 快速粘贴浮层、多组快捷键、区域截图、OCR 图片搜索、文本转换和备份导入导出。
- 六套 CSS 主题；隐藏窗口时降低 WebView 内存占用，可配置空闲销毁与 GPU 行为。
- 新捕获的文本保留首尾空白和原始换行，预览与搜索单独规范化；X11 优先使用 XFixes，必要时回退到包含图片的轮询。
- “设置 → 剪贴板设置 → 不记录这些应用”可排除后续来源应用捕获，每行一个完整应用名、可执行文件名或路径，忽略大小写，可省略 `.exe`，离开输入框后生效。已有历史不受影响，无法识别的来源不能可靠排除。

复制内容后，用配置的全局快捷键打开历史。Quick Paste 默认快捷键为 `Ctrl+Shift+V`，用 ↑/↓ 选择、Enter 粘贴、Esc 关闭。捕获需要主程序运行；仅打开 CLI 不会启动后台桌面监听。

当前桌面支持 Windows 和 Linux X11。完整 Wayland 支持暂缓；OSC52 文本复制属于终端能力，不代表 Wayland 后台监听、全局快捷键和自动粘贴已适配。项目不提供云同步。

## CLI：历史与当前剪贴板

```bash
dzc list --metadata --json --exclude-sensitive
dzc search "project" --ids
dzc get 142 --raw
dzc copy 142 --quiet
dzc clipboard read
printf 'hello\n' | dzc clipboard write -
dzc add "常用片段"
dzc tag add 142 work
dzc export backup.json
```

`list/search/get/add` 操作历史数据库，GUI 关闭时也能工作；`clipboard read/write` 操作当前系统剪贴板。`copy ID` 按记录格式恢复文本、HTML、图片或文件，不自动向应用发送粘贴按键。Windows/X11 原生读写无需 `xclip`、`xsel` 或 `wl-copy`；X11 写入后由轻量后台进程持有内容，被其他应用替换后退出。原生剪贴板需要可访问的桌面会话，Linux GUI 自动粘贴按键仍依赖 `xdotool`。

`--raw` 不补换行；`--mime image/png` 可输出图片字节；列表/搜索支持 JSON、元数据、TSV 和过滤后分页。`--exclude-sensitive` 在解密前排除已标记敏感或加密记录，无法识别所有未标记的私人内容。备份内嵌图片原始字节；文件列表、视频及 HTML 外链资源仍引用原文件。完整参数见 [CLI 文档](./docs/cli.md)。

## fzf、dmenu、rofi 与 TUI

以下适配器需要 Python 3.10+。独立 CLI 压缩包已附带运行所需的 `integrations/` 文件；使用桌面安装的 CLI 时，也可下载同版本源码取得该目录。命令从压缩包或仓库根目录运行，其他位置请使用完整路径。

```bash
# fzf：选择后复制；选择器需另行安装
python3 integrations/dzc-integrations.py pick --backend fzf --action copy
# dmenu / rofi 的逐行菜单
python3 integrations/dzc-integrations.py pick --backend dmenu --action copy
python3 integrations/dzc-integrations.py pick --backend rofi --action copy
# 终端管理界面
python3 integrations/dzc-integrations.py tui --action copy
# Rofi script mode
rofi -show dzc -modi 'dzc:python3 /absolute/path/to/integrations/dzc-integrations.py rofi'
```

`--action print` 输出选中的原始文本，`id` 输出 ID，`copy` 恢复原生剪贴板，`osc52` 发往控制终端。TUI 支持搜索、正文预览、翻页、置顶、标签和确认后删除；↑/↓ 选择，`/` 搜索，Enter 执行动作，`q` 退出。默认动作是 `print`。

Windows 使用 `python`。只有 TUI 需要额外的 curses 兼容依赖：

```powershell
python -m pip install -r integrations/requirements-tui.txt
python integrations/dzc-integrations.py tui --action copy
```

## 插入 Shell 输入行：Bash、Zsh、Fish、PowerShell、Nushell

加载对应脚本后按 `Ctrl-G`，搜索并将记录插入当前输入行，保留多行及末尾换行，等待用户确认。脚本不执行选中的内容，也不自动按 Enter。默认使用 fzf；可提前设置 `DZC_PICKER=rofi` 或 `dmenu`。

```bash
# 按当前 Shell 选择一行；路径换成实际解压目录
source /absolute/path/to/integrations/shell/dzc.bash
source /absolute/path/to/integrations/shell/dzc.zsh
source /absolute/path/to/integrations/shell/dzc.fish  # Fish 3.4+
```

```powershell
# PowerShell 7 + PSReadLine
. 'C:/Tools/dzc/integrations/shell/dzc.ps1'
```

```nu
# Nushell 0.116+；Windows 默认 python，其他平台默认 python3
$env.DZC_BIN = '/absolute/path/to/dzc'
source /absolute/path/to/integrations/shell/dzc.nu
# Windows 示例：source 'C:/Tools/dzc/integrations/shell/dzc.nu'
```

需要永久启用时，自行把对应加载行放入 Shell 配置；Nu 可用 `config nu` 打开配置。脚本不会修改启动文件。还可设置 `DZC_ADAPTER` 和 `DZC_PYTHON` 覆盖适配器/Python 路径；具体语法见 [集成指南](./integrations/README.md#shell-编辑缓冲区集成)。

## SSH：把远程文本复制到本地

远程主机安装独立 `dzc`，本地使用支持并允许 OSC52 的终端连接：

```bash
ssh -t user@server
# 以下在远程执行
dzc copy 142 --target osc52
printf 'remote note\n' | dzc clipboard write - --target osc52
python3 /opt/dzc/integrations/dzc-integrations.py pick --action osc52
```

`native` 目标默认操作远程可访问的桌面剪贴板；显式 `osc52` 将文本发给本地终端，无需远程 GUI 或 `DISPLAY`，不会自动回退。它不读取本地剪贴板、不自动粘贴，也不同步两端历史。最多发送 100000 个原始 UTF-8 字节，超出时报错而不截断；不支持图片/文件，终端权限和容量仍可能阻止写入。成功仅表示序列已发送。

OSC52 写入控制终端，标准输出保持为空；没有控制终端时报错。若运行在一层 tmux 中并需要穿透，显式使用 `--osc52-passthrough tmux`；tmux 3.3+ 还需用户自行允许 `allow-passthrough`。程序不会修改终端或 tmux 配置。示例与限制见 [OSC52 集成说明](./integrations/README.md#ssh-与-osc52)。

## 只读 MCP 与 Agent Skill

在 MCP 客户端中配置绝对路径：

```json
{
  "mcpServers": {
    "dezirclip": {
      "command": "python3",
      "args": ["/absolute/path/to/integrations/dzc-integrations.py", "mcp"],
      "env": {"DZC_BIN": "/absolute/path/to/dzc"}
    }
  }
}
```

Windows 将 `command` 改为 Python 的实际路径，并为脚本和 `DZC_BIN` 使用 Windows 路径。服务只通过 stdio 通信，不监听网络端口，提供 `clipboard_list`、`clipboard_search`、`clipboard_get`。列表和搜索只返回元数据，正文需单独请求；敏感标签和加密记录在 CLI 解密前过滤，读取结果有大小上限。它不提供写入、复制、删除或执行工具。

无需 MCP 的 Agent 也可安装 [`skills/dzc-cli`](./skills/dzc-cli/) 的技能文档来使用 CLI；技能安装脚本只安装说明，不替代二进制。可用参数、返回值和完整配置见 [集成指南](./integrations/README.md#只读-mcp-stdio-服务)。

## 开发与验证

使用 Node.js 22.12+、项目固定的 pnpm 11.0.9 和稳定版 Rust；Windows 还需 C++ 构建工具与 WebView2，Linux 构建依赖见 [安装指南](./docs/LINUX_INSTALL.md)。

```bash
pnpm install
pnpm run tauri:dev
pnpm run build
pnpm run i18n:check
pnpm run test:unit
cargo test --manifest-path src-tauri/Cargo.toml
python3 -m unittest discover -s integrations/tests -v
python3 -m unittest discover -s scripts/tests -v
pnpm run tauri:build
```

桌面构建产物在 `src-tauri/target/release/bundle/`。只构建 CLI 可运行 `cargo build --manifest-path src-tauri/Cargo.toml --bin dzc --release`。使用 Python 3.11+ 的 `scripts/package-dzc.py --platform windows-x86_64` 等参数打包真实 CLI；`scripts/verify-dzc-package.py 压缩包路径` 会解压后检查独立进程间的数据持久化。发布流程与安装命令以仓库脚本及对应版本说明为准；旧 roadmap 仅作为历史规划，不是当前功能清单。

## 许可证与来源

DezirClip 使用 GPL-3.0，源自同样使用 GPL-3.0 的 [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard)。适用的原始版权和许可证声明予以保留。

## English

DezirClip is a local clipboard workspace for Windows and Linux X11, forked from [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard). This fork removes macOS and cloud/network features, adds X11 support, custom CSS themes and WebView memory controls, and provides a shared Rust storage core for the GUI and `dzc` CLI. Full Wayland desktop support remains deferred.

Download desktop installers or standalone CLI archives from [Releases](https://github.com/qiyuanhuakai/dezirclip/releases). Standalone archives are named `dzc-VERSION-{windows,linux}-{x86_64,aarch64}` with `.zip` on Windows or `.tar.gz` on Linux; each includes the real CLI, license, matching `integrations/`, and Agent Skill. Check the accompanying `.sha256`. Official Linux CLI builds require glibc 2.39+ and their dynamic libraries. Older releases may lack newer flags; keep CLI and adapters on matching versions.

Add the extracted binary directory to `PATH`, or set `DZC_BIN` to its absolute path. History commands work without the GUI; native clipboard access needs an accessible desktop session. Ordinary host installations share the current user's history; Flatpak defaults are sandboxed, and AppImage mount paths are temporary. See the [CLI reference](./docs/cli.md) for stable installation paths, sandbox access and portable data locations. Archives contain all integration runtime files, excluding development tests.

```bash
dzc list --metadata --json --exclude-sensitive
dzc search "project" --ids
dzc get 142 --raw
dzc copy 142 --quiet
dzc clipboard read
printf 'hello\n' | dzc clipboard write -
```

The GUI offers history, tags, pinning, rich text, images, files, OCR search, screenshots, text transforms and themes. Quick Paste defaults to Ctrl+Shift+V; use arrows and Enter to paste, or Esc to close. New captures preserve original whitespace and line endings. X11 uses XFixes with image-aware polling fallback. Source application exclusions block future captures when the source can be identified.

Optional adapters need Python 3.10+; fzf/dmenu/rofi are installed separately. On Windows use `python`; only the TUI additionally needs `python -m pip install -r integrations/requirements-tui.txt`.

```bash
python3 integrations/dzc-integrations.py pick --backend fzf --action copy
python3 integrations/dzc-integrations.py pick --backend dmenu --action copy
python3 integrations/dzc-integrations.py pick --backend rofi --action copy
python3 integrations/dzc-integrations.py tui --action copy
rofi -show dzc -modi 'dzc:python3 /absolute/path/to/integrations/dzc-integrations.py rofi'
```

Source the script for your shell, then press Ctrl-G to insert selected text into the current editing buffer without executing it. Bash, Zsh, Fish 3.4+, PowerShell 7 with PSReadLine, and Nushell 0.116+ are supported. Multiline text and trailing newlines are preserved. Add source lines to startup configuration yourself if desired; scripts do not change those files.

```bash
source /absolute/path/to/integrations/shell/dzc.bash
source /absolute/path/to/integrations/shell/dzc.zsh
source /absolute/path/to/integrations/shell/dzc.fish
```

```powershell
. 'C:/Tools/dzc/integrations/shell/dzc.ps1'
```

```nu
source /absolute/path/to/integrations/shell/dzc.nu
```

For SSH, install `dzc` remotely and connect with a terminal that permits OSC52:

```bash
ssh -t user@server
dzc copy 142 --target osc52
printf 'remote note\n' | dzc clipboard write - --target osc52
python3 /opt/dzc/integrations/dzc-integrations.py tui --action osc52
```

OSC52 writes text to the controlling terminal, keeping stdout empty; no remote display is needed. It neither reads the local clipboard nor syncs history. The fixed limit is 100000 original UTF-8 bytes; oversized payloads fail without truncation. Terminal support, permissions and lower limits still apply; success means sent, not acknowledged. Native clipboard access remains the default with no automatic OSC52 fallback. Explicit single-layer tmux passthrough uses `--osc52-passthrough tmux` and may require user-configured `allow-passthrough`; see the [integration guide](./integrations/README.md#ssh-与-osc52).

For read-only MCP, use `python3 /absolute/path/to/integrations/dzc-integrations.py mcp` as the server command, or the JSON configuration above. It exposes bounded list/search/get tools over stdio, excludes sensitive/encrypted records before decoding, and provides no mutation or execution tools. The alternative [Agent Skill](./skills/dzc-cli/) documents direct CLI use. See [integrations](./integrations/README.md) for configuration and [Linux installation](./docs/LINUX_INSTALL.md) for dependencies and troubleshooting.

Build with the commands in the development section above. DezirClip and its upstream are licensed under GPL-3.0; applicable original copyright and license notices are retained.
