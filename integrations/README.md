# 可选的 dzc 终端与 Agent 集成

这些适配器通过本地 `dzc` 子进程工作，不增加桌面端依赖，也不启动网络服务。需要 Python 3.10+ 与本次版本的 `dzc`；选择器另需 fzf、rofi 或 dmenu。以下路径均从仓库根目录运行，路径有空格时加引号。

```powershell
python integrations/dzc-integrations.py --dzc C:/path/to/dzc.exe pick
```

Linux 通常用 `python3`。`dzc` 已在 PATH 时省略 `--dzc`；也可设置 `DZC_BIN` 为可执行文件的完整路径。所有调用使用独立参数，不把记录内容拼入 shell 命令。适配器与 CLI 需一同更新，因为它们使用 `--metadata`、分页与 `--exclude-sensitive`。

## fzf / dmenu / rofi 选择器

```bash
python3 integrations/dzc-integrations.py pick
python3 integrations/dzc-integrations.py pick --backend dmenu --action copy
python3 integrations/dzc-integrations.py pick --backend rofi --query meeting --action copy
python3 integrations/dzc-integrations.py pick --kind text --limit 200 --action id
```

默认获取 100 条元数据（最多 200 条），每行显示类型、置顶标记与摘要；选中后才读取内容。`print` 输出原始文本，没有额外换行；`copy` 通过原生系统剪贴板恢复记录，适合图片与文件；`id` 只输出 ID。Shell 使用 `insert`，与 `print` 相同但拒绝编辑缓冲区无法表示的 NUL 字节。取消返回 1，Ctrl-C 返回 130，不输出记录。为使行为可预测，fzf 不读取 `FZF_DEFAULT_OPTS` / `FZF_DEFAULT_OPTS_FILE`。

Rofi 的 script mode 也可直接使用，选择后复制记录：

```bash
rofi -show dzc -modi 'dzc:python3 /absolute/path/to/integrations/dzc-integrations.py rofi'
```

含空格的安装路径可写入一个普通启动脚本，再将其路径传给 `-modi`。支持 rofi 的 `ROFI_RETV` 与隐藏 `info` 字段；自定义输入被禁用。dmenu 兼容模式使用普通的逐行文本，不依赖 rofi 专有字段。

## 终端浏览器

```bash
python3 integrations/dzc-integrations.py tui
python3 integrations/dzc-integrations.py tui --query project --action copy
```

Linux 使用 Python 自带的 curses。Windows 仅为 TUI 安装下面的可选依赖；MCP、选择器和 shell 集成都不需要它：

```powershell
python -m pip install -r integrations/requirements-tui.txt
python integrations/dzc-integrations.py tui
```

| 按键 | 操作 |
| --- | --- |
| ↑/↓ 或 j/k | 选择记录，显示完整内容预览 |
| / | 搜索；空查询返回全部 |
| h/l 或 ←/→ | 滚动内容预览 |
| PageUp/PageDown 或 [ / ] | 前后翻页 |
| p | 切换置顶 |
| t / u | 添加 / 移除标签 |
| d | 删除；必须再次输入 `yes` |
| c | 复制到系统剪贴板，继续浏览 |
| Enter | 执行 `--action` 并退出，默认打印文本 |
| r / q / Esc | 刷新 / 退出 |

TUI 在控制终端绘制，所以重定向标准输出时不会混入界面字符。元数据每页最多 200 条；预览最多显示 64,000 字符。适配器限制单次 `dzc` 输出为 4 MiB，超过大小会明确报错，可直接使用 `dzc` 处理大记录。终端预览与选择器摘要会清除控制字符；打印选中的文本仍保留原始内容。

## Shell 编辑缓冲区集成

对应脚本写入 shell 的**当前编辑缓冲区**，保留多行和末尾换行；不会调用 `eval` 或自动提交命令。绑定为 Ctrl-G，由用户检查后决定如何使用。需要 fzf，或在加载前将 `DZC_PICKER` 设置为 `rofi` / `dmenu`。

```bash
# Bash
source /absolute/path/to/integrations/shell/dzc.bash
# Zsh
source /absolute/path/to/integrations/shell/dzc.zsh
# Fish 3.4+
source /absolute/path/to/integrations/shell/dzc.fish
```

```powershell
# PowerShell 7 + PSReadLine
. C:/absolute/path/to/integrations/shell/dzc.ps1
```

加载前可设置 `DZC_ADAPTER`（适配器完整路径）、`DZC_PYTHON`（Python 可执行文件）和 `DZC_PICKER`。脚本不会安装文件或修改 shell 启动配置。按需把 `source` / 点加载语句加入自己的配置文件。

## 只读 MCP stdio 服务

```bash
python3 /absolute/path/to/integrations/dzc-integrations.py mcp
```

MCP 客户端配置示例，按自己的绝对路径替换：

```json
{
  "mcpServers": {
    "dezirclip": {
      "command": "python3",
      "args": ["/absolute/path/to/integrations/dzc-integrations.py", "mcp"]
    }
  }
}
```

Windows 将 `command` 设为 Python 的绝对路径，并在 `args` 中使用 Windows 路径；可通过客户端的 `env.DZC_BIN` 指定 `dzc.exe`。服务仅使用标准输入输出，不监听端口。标准输出只包含协议消息。关闭标准输入即可结束服务。

| 工具 | 参数 | 返回 |
| --- | --- | --- |
| `clipboard_list` | `limit` 1–100，`offset` | 元数据与 `next_offset` |
| `clipboard_search` | `query`，可选 `limit`、`offset` | 包含搜索的元数据 |
| `clipboard_get` | 数字 `id`，可选 `max_chars` 1–16384 | 有界内容与 `truncated` 标记 |

默认每页 20 条，仅列表和搜索返回最多 500 字符的摘要；完整内容需明确调用 `clipboard_get`，默认上限 4096 字符。敏感标签和加密记录通过 CLI 的 `--exclude-sensitive` 在解密前过滤。适配器还验证敏感标记，且只转发固定字段。不存在开放全部历史或绕过隐私过滤的配置，也不提供写入、复制、删除或 shell 执行工具。每次 CLI 请求超时 30 秒，协议输入单条上限 64 KiB。

协议实现参考 MCP 2025-11-25 的 [stdio 传输](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、[初始化生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) 与 [tools 规范](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。也支持客户端协商 2025-06-18、2025-03-26、2024-11-05。剪贴板内容始终作为不可信数据返回；工具说明不会将记录内容当作指令。

## 验证

```bash
python3 -m unittest discover -s integrations/tests -v
```

这些测试独立于桌面端，覆盖命令参数、MCP 协议生命周期、权限边界、隐私过滤、分页、内容截断、选择取消和控制字符清理。它们不读取用户的真实剪贴板或历史。

设置 `DZC_TEST_BIN` 后还会运行实际 CLI 与 MCP 子进程测试，全部使用临时数据库：

```powershell
$env:DZC_TEST_BIN = (Resolve-Path src-tauri/target/debug/dzc.exe).Path
python -m unittest discover -s integrations/tests -v
```

```bash
DZC_TEST_BIN="$PWD/src-tauri/target/debug/dzc" python3 -m unittest discover -s integrations/tests -v
```

实际 CLI 检查包括原样输出的 CRLF / Unicode / 末尾换行、过滤后的分页、长内容正则与模糊搜索、敏感与加密记录过滤、只读 MCP 的完整初始化与调用，以及备份的长文本/HTML 保真、稀疏 ID 保留、替换行为与非法版本拒绝。Linux 还用真实终端检查 TUI 输出隔离，并在已安装的 Bash / Zsh / Fish 中验证编辑缓冲区插入。Windows 可选安装 `pywinpty` 后，测试通过隐藏的 ConPTY 运行真实 TUI 与 PowerShell 7 PSReadLine，验证重定向输出及多行文本仅插入编辑缓冲区；TUI 测试还需 `windows-curses`。这些测试依赖不属于桌面应用或适配器运行时。
