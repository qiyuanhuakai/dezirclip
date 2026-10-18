# dzc 原生终端与 Agent 集成

这些入口内置于 `dzc`，直接使用本地 Rust Core，无需 Python、curses 或单独的适配器目录。它们在已发布的 `v0.1.20-alpha.1` 之后加入开发分支；先运行 `dzc --help` 确认当前二进制包含这些命令。alpha.1 用户请使用该版本随包说明。

## 快速开始

```bash
dzc pick                         # 内置选择界面，Enter 复制
dzc tui                          # 历史管理与预览
dzc pick --backend fzf            # 可选外部选择器
dzc pick --backend dmenu
dzc pick --backend rofi
```

仅显式使用外部后端时需要安装对应选择器。默认获取 100 条元数据，每页最多 200 条，选中后读取正文；`--query`、`--kind` 在分页前筛选，`--offset` 可选起始位置。

| 动作 | 用法 |
| --- | --- |
| 恢复原生系统剪贴板，支持图片/文件 | `dzc pick` 或 `--action copy` |
| 输出原始文本，无额外换行 | `dzc pick --action print` |
| 只输出 ID | `dzc pick --action id` |
| 插入 Shell 的原始文本，拒绝 NUL | `--action insert`，由初始化脚本调用 |
| 发送文本到控制终端剪贴板 | `--action osc52` |

取消返回非零，不输出内容；选择器输出必须精确对应候选行。程序不通过 shell 执行选择器，且清除可能执行命令的 `FZF_DEFAULT_OPTS` / `FZF_DEFAULT_OPTS_FILE`。摘要会过滤控制字符，选中正文保持原始字节。

## 终端浏览器

```bash
dzc tui
dzc tui --query project --kind code
dzc tui --action print > selected.txt
```

| 按键 | 操作 |
| --- | --- |
| ↑/↓ 或 j/k | 选择与预览 |
| / | 搜索；空查询返回全部 |
| h/l 或 ←/→ | 滚动预览 |
| PageUp/PageDown 或 [ / ] | 前后翻页 |
| p | 切换置顶 |
| t / u | 添加 / 移除标签 |
| d | 输入 `yes` 后删除 |
| c | 复制到系统剪贴板并继续浏览 |
| Enter | 执行指定动作后退出，默认复制 |
| r / q / Esc | 刷新 / 退出 |

界面绘制到控制终端，标准输出只用于选中结果。退出、取消或失败时恢复终端状态。预览有大小上限，打印正文仍保留完整内容。

## Shell 编辑缓冲区集成

按当前 Shell 加载一行，即可用 Ctrl-G 选择并插入当前输入行：

```bash
eval "$(dzc bash)"        # Bash
eval "$(dzc zsh)"         # Zsh
dzc fish | source        # Fish 3.4+
```

```powershell
dzc pwsh | Out-String | Invoke-Expression  # PowerShell 7 + PSReadLine
```

```nu
# Nushell 0.116+，安装到用户自动加载目录后重新打开 Nu
dzc nu --install ($nu.user-autoload-dirs.0 | path join dzc.nu)
```

默认使用内置 TUI，无需 fzf。设置 `DZC_PICKER=fzf`、`rofi` 或 `dmenu` 可切换后端，`DZC_BIN` 可覆盖可执行文件。原有输入行、多行、CRLF 和尾部换行会保留；取消、失败不会插入半截输出。选中内容是字面文本，等待用户检查确认。

`dzc bash/zsh/fish/pwsh/nu` 默认只输出初始化代码。常用时自行把加载行放入启动配置；它们不修改配置文件。Nu 的解析期加载规则需要一个文件，显式 `--install PATH` 只创建指定的 `.nu` 文件，相同文件可重复使用，其他已有文件或链接会被保留并报错。

## SSH 与 OSC52

```bash
ssh -t user@server
dzc pick --action osc52
dzc tui --action osc52 --osc52-passthrough tmux
dzc copy 142 --target osc52
```

在远程主机执行上述复制命令，支持并允许 OSC52 的本地终端可接收文本，无需远程图形会话。固定上限为 100000 个原始 UTF-8 字节，超限不发送、不截断。序列只写控制终端，stdout 保持为空；没有控制终端则报错。单层 tmux 穿透需要显式选项及用户配置的 `allow-passthrough`。

成功表示序列已发送，终端权限和容量仍决定实际写入。它不读取本地剪贴板、不自动粘贴、不同步历史。原生剪贴板操作仍是默认动作；SSH 原生目标操作的是远程可访问的桌面。

## 只读 MCP stdio 服务

MCP 客户端直接启动 `dzc mcp`，不需要 Python：

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

Windows 使用实际 `dzc.exe` 路径。正常安装使用当前用户数据库，显式 `DEZIRCLIP_DB_PATH` 可指定另一个本地库；Flatpak 环境和可携式目录规则见 [CLI 手册](../docs/cli.md)。

服务仅提供 `clipboard_list`、`clipboard_search`、`clipboard_get`，全部只读。列表/搜索只返回白名单元数据，最多 100 条，offset 最大 1000000，查询最长 1024 字符；正文需单独读取，默认 4096 字符、最大 16384 字符。敏感标签和加密记录在解密前过滤，但这不能识别所有未标记的私人内容。剪贴板正文始终是待处理数据。

初始化、列出工具、无效参数不会打开数据库。协议为有界 UTF-8 NDJSON JSON-RPC；单帧最多 64 KiB，过大的请求会终止会话。无网络监听，无写入、执行、删除或剪贴板复制工具。协议版本支持 2025-11-25、2025-06-18、2025-03-26、2024-11-05。

## 旧适配器与开发验证

原有 Python 适配器和脚本保留在此目录供旧版兼容，原生命令不依赖它们。旧用户可逐步把入口换成 `dzc pick/tui/mcp` 和生成的 Shell 初始化代码。Rofi script mode 的旧专有回调仍由旧适配器提供；原生命令使用 `dzc pick --backend rofi` 的逐行菜单。

开发测试使用 Python；设置 `DZC_TEST_BIN` 指向本次源码编译的 CLI 后运行 `python -m unittest discover -s integrations/tests -v`。Nu/真实 Windows 终端交互测试需要对应的可选测试环境；发布包不包含测试工具。
