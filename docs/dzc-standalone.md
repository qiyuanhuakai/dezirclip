# dzc 独立命令行工具

本压缩包包含 DezirClip 的完整 CLI、匹配版本的可选适配器和 Agent Skill。无需启动桌面主程序即可管理本地 SQLite 历史。`dzc` 不联网，不自动同步其他机器的历史，也不在 CLI 运行时启动桌面捕获服务。

Windows 独立 CLI 无需 WebView2；当前 MSVC 构建需要 `VCRUNTIME140.dll`。压缩包不附带该文件，干净系统请先安装匹配架构的 [Microsoft Visual C++ v14 运行库](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)：[x64](https://aka.ms/vc14/vc_redist.x64.exe) / [ARM64](https://aka.ms/vc14/vc_redist.arm64.exe)。

解压后将 `dzc`（Windows 为 `dzc.exe`）所在目录加入 `PATH`，或直接使用完整路径：

```bash
dzc --version
dzc list --metadata --json
dzc add "hello"
dzc get latest --raw
```

同一用户的普通安装与桌面端共享默认数据库；CLI 也遵循桌面端默认数据目录里的 `datapath.txt` 自定义目录指向。若 CLI 可执行文件旁存在 `data/` 目录，则优先使用其中的 `clipboard.db`，形成该解压目录独立的可携式数据。`DEZIRCLIP_DB_PATH` 显式指定数据库时优先级最高，旧别名 `TIEZ_DB_PATH` 仅在前者未设置时使用。重复解压多个 CLI 不会自动合并多份数据库。完整位置与命令语义见 [CLI 文档](./docs/cli.md)。

使用匹配版本的桌面端与 CLI。遇到由更新版本创建、超出当前支持范围的数据库结构时，程序会拒绝打开并要求升级，不会尝试降级或覆盖原数据。

Flatpak 沙箱的数据目录不同于宿主机独立 CLI 的默认目录，两者不会自动共享历史。访问 Flatpak 应用的历史时，使用其沙箱内附带的 CLI，例如：

```bash
flatpak run --command=dzc io.github.qiyuanhuakai.dezirclip list --metadata --json
```

AppImage 内的 CLI 位于临时挂载目录，不适合作为永久 `PATH` 软链接；需要稳定终端、Shell 插件或 MCP 路径时，使用本压缩包并解压到固定目录。

Windows 当前会话示例：

```powershell
$env:Path = "C:\Tools\dzc;$env:Path"
$env:DZC_BIN = 'C:\Tools\dzc\dzc.exe'
dzc pick
```

原生 `dzc pick`、`dzc tui`、`dzc mcp` 和 `dzc bash/zsh/fish/nu/pwsh` 已内置于 CLI，无需 Python 或单独的脚本目录。默认选择界面和 Ctrl-G 插件使用内置 TUI；只有 `--backend fzf/dmenu/rofi` 需要对应的外部选择器。Shell 加载与 MCP 设置见 [集成指南](./integrations/README.md)。这些原生入口自 `v0.1.20-alpha.2` 起提供；旧包应使用自己的随包说明。

原生剪贴板读写需要当前系统的图形会话；Linux 官方二进制基于 Ubuntu 24.04，要求 glibc 2.39+ 及其动态库，不是静态兼容构建。在 SSH 终端中，可显式通过 OSC52 将远程文本复制到支持并允许该功能的本地终端：

```bash
dzc copy 142 --target osc52
```

此操作无需远程 GUI，但不读取剪贴板或同步历史；最多 100000 个原始 UTF-8 字节。终端支持和权限仍决定是否实际写入。详见集成指南的 SSH 章节。

DezirClip 是 [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard) 的本地 Windows/Linux X11 分支，采用 GPL-3.0。压缩包内 `LICENSE` 保留适用许可证。源码与发布版本见 [项目仓库](https://github.com/qiyuanhuakai/dezirclip)。
