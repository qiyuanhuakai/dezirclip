# tiez-c CLI 使用文档

`tiez-c` 是 TieZ 剪贴板管理器的命令行界面工具。通过终端即可快速查看、搜索、添加、导出剪贴板历史记录，适合与 fzf、shell 脚本或其他自动化工具配合使用。

## 安装

### Linux

#### 通过包管理器安装

**Ubuntu / Debian（deb 包）**

从 GitHub Releases 下载最新 `.deb` 包后安装：

```bash
wget https://github.com/your-org/tiez-clipboard/releases/latest/download/tiez-c_linux_amd64.deb
sudo dpkg -i tiez-c_linux_amd64.deb
```

依赖通过 `libgtk-3-dev` 和 `libwebkit2gtk-4.0-dev` 满足。如果缺少 `libssl` 相关依赖，执行：

```bash
sudo apt install -y libssl-dev
```

**Arch Linux（手动安装）**

Arch 用户可直接从 AUR 或手动放置二进制到 `/usr/local/bin/`：

```bash
sudo cp tiez-c /usr/local/bin/
sudo chmod +x /usr/local/bin/tiez-c
```

依赖确认：

```bash
sudo pacman -S gtk3 webkit2gtk libappindicator-gtk3 xdotool
```

#### 手动下载

访问 [Releases 页面](https://github.com/your-org/tiez-clipboard/releases)，下载对应架构的压缩包（`x86_64-unknown-linux-gnu.tar.gz`），解压后将 `tiez-c` 放入 PATH：

```bash
tar xzf tiez-c-linux-x86_64.tar.gz
sudo mv tiez-c /usr/local/bin/
```

安装后验证：

```bash
tiez-c --version
```

### Windows

#### 通过 Scoop 安装

```powershell
scoop bucket add tiez https://github.com/your-org/tiez-clipboard-scoop
scoop install tiez-c
```

#### 手动下载

从 Releases 页面下载 `tiez-c-windows-x64.zip`，解压后将 `tiez-c.exe` 放入任意 PATH 目录（如 `C:\Tools`）。

安装后验证：

```powershell
tiez-c --version
```

## 命令速查表

| 命令 | 用途 | 常用选项 |
|------|------|----------|
| `list [N]` | 列出最近 N 条剪贴板记录（默认 10） | `--type`, `--tag`, `--pinned`, `--json`, `--ids`, `--quiet` |
| `search <QUERY>` | 按关键词搜索 | `--mode`（contains / fuzzy / regex / fts5） |
| `get <ID\|latest>` | 查看单条记录详情 | `--preview` |
| `add <CONTENT>` | 手动添加文本/HTML/图片 | `--type`（text / html / image） |
| `delete <ID>` | 删除指定记录 | 无 |
| `pin <ID>` | 置顶记录 | 无 |
| `unpin <ID>` | 取消置顶 | 无 |
| `tag add <ID> <TAG>` | 为记录添加标签 | 无 |
| `tag remove <ID> <TAG>` | 移除记录标签 | 无 |
| `tag list <ID>` | 列出记录的所有标签 | 无 |
| `export <PATH>` | 导出剪贴板历史 | `--encrypted`, `--passphrase` |
| `import <PATH>` | 导入历史文件 | `--mode`（merge / replace） |
| `stats` | 显示统计摘要 | 无 |
| `watch` | 实时监控新记录 | `--pattern`, `--notify` |

## tiez-c list

列出最近的剪贴板历史记录。

### 基本用法

```bash
tiez-c list
tiez-c list 20
```

### 过滤选项

```bash
# 只看文本类型
tiez-c list --type text

# 按标签过滤
tiez-c list --tag todo

# 只看置顶记录
tiez-c list --pinned
```

### 输出格式

```bash
# JSON 输出，供脚本解析
tiez-c list --json

# 只输出 ID，每行一个，配合 fzf 使用
tiez-c list --ids

# 安静模式，只输出内容本身
tiez-c list --quiet
```

### 组合过滤

```bash
tiez-c list 50 --type text --tag work --json
```

## tiez-c search

在剪贴板历史中搜索关键词。

### 基本用法

```bash
tiez-c search "TODO"
tiez-c search "function main"
```

### 搜索模式

```bash
# 包含匹配（默认）
tiez-c search "fix bug" --mode contains

# 模糊匹配（拼写容错）
tiez-c search "fuction" --mode fuzzy

# 正则匹配
tiez-c search "href=.*example\\.com" --mode regex

# FTS5 全文搜索（对大文本更高效）
tiez-c search "important meeting notes" --mode fts5
```

### 搜索并导出

```bash
tiez-c search "password" --json > matches.json
```

## tiez-c get

查看指定剪贴板记录的完整内容。

### 基本用法

```bash
# 按 ID 查看
tiez-c get abc123def

# 查看最新记录
tiez-c get latest
```

### 预览模式

```bash
# 仅显示前 200 个字符，适合长文本
tiez-c get abc123def --preview
```

### 结合 list 使用

```bash
tiez-c list --ids | fzf | xargs tiez-c get
```

## tiez-c add

手动向剪贴板历史添加一条记录。

### 基本用法

```bash
# 添加纯文本
tiez-c add "Hello, World!"

# 添加多行文本
tiez-c add "第一行
第二行
第三行"
```

### 指定类型

```bash
# HTML 格式
tiez-c add "<p>你好，<b>世界</b></p>" --type html

# 图片（从文件读取）
tiez-c add --type image /path/to/screenshot.png
```

### 从标准输入读取

```bash
echo "从管道输入" | tiez-c add
cat report.txt | tiez-c add --type text
```

## tiez-c delete

删除指定的剪贴板记录。

### 基本用法

```bash
# 删除单条记录
tiez-c delete abc123def
```

### 批量删除

```bash
# 先获取 ID 列表，再删除
tiez-c list --ids | fzf -m | xargs -I {} tiez-c delete {}
```

## tiez-c pin / unpin

固定重要记录，防止被自动清理。

### 置顶

```bash
tiez-c pin abc123def
```

### 取消置顶

```bash
tiez-c unpin abc123def
```

### 查看所有置顶记录

```bash
tiez-c list --pinned
```

## tiez-c tag

管理剪贴板记录的标签，方便分类和过滤。

### 添加标签

```bash
tiez-c tag add abc123def todo
tiez-c tag add abc123def work project-x
```

### 移除标签

```bash
tiez-c tag remove abc123def todo
```

### 列出标签

```bash
# 列出某条记录的所有标签
tiez-c tag list abc123def

# 列出所有已使用的标签（通过 list + 去重实现）
tiez-c list --json | jq -r '.[].tags[]?' | sort -u
```

## tiez-c export / import

导出和导入剪贴板历史，支持加密导出。

### 导出

```bash
# 导出为明文 JSON
tiez-c export /tmp/backup.json

# 加密导出
tiez-c export /tmp/backup.tiez --encrypted --passphrase "my-strong-pass"
```

### 导入

```bash
# 合并模式：导入内容追加到现有历史
tiez-c import /tmp/backup.json --mode merge

# 替换模式：清空现有历史后导入（谨慎使用）
tiez-c import /tmp/backup.json --mode replace
```

### 导入加密备份

```bash
tiez-c import /tmp/backup.tiez --mode merge --passphrase "my-strong-pass"
```

## tiez-c stats

查看剪贴板使用统计。

```bash
tiez-c stats
```

输出示例：

```
总记录数：    1,234
文本记录：    1,100
HTML 记录：   50
图片记录：    84
置顶记录：    12
标签总数：    37
最常使用标签：work (234), todo (189), personal (156)
```

### JSON 格式统计

```bash
tiez-c stats --json
```

## tiez-c watch

实时监控剪贴板变化，并在匹配条件时发出通知。

### 基本用法

```bash
# 监控所有新记录
tiez-c watch
```

### 按模式过滤

```bash
# 仅监控包含 "TODO" 的记录
tiez-c watch --pattern "TODO"
```

### 通知模式

```bash
# 匹配时发送桌面通知
tiez-c watch --pattern "ERROR" --notify
```

### 结合通知工具

```bash
# Linux 使用 notify-send
tiez-c watch --pattern "urgent" --notify --cmd 'notify-send "tiez-c" "$TIEMATCH"'

# Windows 使用 toast
tiez-c watch --pattern "urgent" --notify --cmd 'powershell -Command "New-BurntToastNotification -Text \"tiez-c\", \"$TIEMATCH\""'
```

### 静默监控

```bash
# 只输出匹配的内容，不输出其他日志
tiez-c watch --pattern "password" --quiet
```

## 常用模式示例

### fzf 集成

通过 `fzf` 交互式选择剪贴板记录，然后查看详情：

```bash
tiez-c list --ids | fzf --prompt="选择剪贴板记录 > " | xargs tiez-c get
```

与预览窗口结合，显示内容摘要：

```bash
tiez-c list --ids | fzf --preview='tiez-c get {} --preview' --preview-window=right:60%
```

### 监控 TODO

在开发过程中监控所有新增的 TODO 项并通知：

```bash
tiez-c watch --pattern "TODO" --notify
```

结合特定标签：

```bash
tiez-c list --tag todo --ids | xargs -I {} sh -c 'tiez-c get {} --preview | grep -i "fixme\|hack\|bug"'
```

### 快速添加

从命令行快速将文本加入剪贴板历史：

```bash
tiez-c add "npm run build"
tiez-c add "docker compose up -d"
```

从当前目录结构快速记录：

```bash
tiez-c add "$(pwd)"
tiez-c add "$(git diff --stat)"
```

### 导出加密备份

创建加密的剪贴板历史备份：

```bash
tiez-c export /home/user/backup.tiez --encrypted --passphrase "$(pass show clipper/backup-pass)"
```

定时任务自动备份：

```bash
0 2 * * * /usr/local/bin/tiez-c export /backup/tiez-$(date +\%Y\%m\%d).tiez --encrypted --passphrase "$BACKUP_PASS"
```

### 跨设备同步（占位）

跨设备同步功能在规划中，当前版本的数据迁移需手动导出导入：

```bash
# 设备 A 导出
tiez-c export /tmp/sync.tiez --encrypted --passphrase "shared-secret"

# 通过 scp / USB 传输 /tmp/sync.tiez 到设备 B

# 设备 B 导入（合并模式）
tiez-c import /tmp/sync.tiez --mode merge --passphrase "shared-secret"
```

> 注意：自动云同步尚未实现。如需多设备无缝同步，请关注后续版本更新。

## 输出格式

### 人类可读输出

默认输出为表格格式，适合终端直接阅读：

```bash
$ tiez-c list 3
ID          时间               类型    标签
─────────────────────────────────────────────────────
a1b2c3d4    2026-01-15 10:23   text    [work]
e5f6g7h8    2026-01-15 09:45   text    [personal]
i9j0k1l2    2026-01-14 18:30   image

```

### JSON 输出

使用 `--json` 获取结构化数据，方便脚本处理：

```bash
tiez-c list --json
```

输出示例：

```json
[
  {
    "id": "a1b2c3d4",
    "timestamp": "2026-01-15T10:23:00Z",
    "type": "text",
    "content": "Hello World",
    "tags": ["work"],
    "pinned": true
  }
]
```

### ID 列表输出

使用 `--ids` 仅输出 ID，每行一个：

```bash
$ tiez-c list --ids
a1b2c3d4
e5f6g7h8
i9j0k1l2
```

### 安静模式

使用 `--quiet` 仅输出内容本身：

```bash
$ tiez-c list --quiet
Hello World
Another clipboard entry
Image data...
```

## 故障排除

### 命令无响应或卡住

**症状：** `tiez-c list` 长时间无输出。

**原因：** 数据库被锁定或 TieZ 主程序未运行。

**解决：**

```bash
# 确认 TieZ 主程序正在运行
ps aux | grep tiez

# 如未运行，启动主程序
tiez &

# 等待 2 秒后重试
sleep 2 && tiez-c list
```

### 导入时提示格式错误

**症状：** `tiez-c import backup.json` 报 "invalid format"。

**原因：** 导入文件不是合法的 tiez-c 导出格式，或文件已损坏。

**解决：**

```bash
# 验证 JSON 格式
jq . backup.json

# 确认文件由 tiez-c export 生成
head -5 backup.json
# 应看到 {"version":"1.0",...}
```

### 搜索无结果但内容确实存在

**症状：** `tiez-c search "foo"` 返回空。

**原因：** 搜索模式不匹配，或内容为 HTML/图片类型。

**解决：**

```bash
# 尝试模糊模式
tiez-c search "foo" --mode fuzzy

# 确认记录类型
tiez-c list --type text --json | jq '.[] | select(.content | test("foo"))'

# 如为图片，text search 不会命中，使用 --type image 确认
tiez-c list --type image
```

### 导出加密文件无法导入

**症状：** 导入时报 "decryption failed"。

**原因：** 密码错误或文件损坏。

**解决：**

```bash
# 确认密码无误
tiez-c export /tmp/test.tiez --encrypted --passphrase "correct-pass"
tiez-c import /tmp/test.tiez --passphrase "correct-pass"

# 如使用 scp 传输，确认文件完整
scp user@host:/path/backup.tiez /tmp/
md5sum /tmp/backup.tiez  # 与源端对比
```

### 通知不显示

**症状：** `tiez-c watch --notify` 运行正常但桌面无通知。

**原因：** 系统通知服务未运行或 tiez-c 未找到通知命令。

**解决：**

```bash
# Linux：确认通知守护进程运行
systemctl --user status dunst  # 或 gnome-shell、xfce4-notifyd

# 手动测试通知
notify-send "test" "tiez-c test notification"

# Windows：确认 Toast 权限
powershell -Command "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null"
```

## 附录：环境变量

`tiez-c` 支持以下环境变量：

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `TIEZ_DB_PATH` | 指定数据库文件路径 | 平台默认 |
| `TIEZ_CONFIG_DIR` | 配置文件目录 | `~/.config/tiez` |
| `TIEZ_NO_NOTIFY` | 禁用所有桌面通知 | 未设置 |
| `TIEZ_EDITOR` | `get` 命令打开编辑器时使用 | `$EDITOR` |

示例：

```bash
export TIEZ_DB_PATH=/mnt/backup/tiez.db
tiez-c list
```

## 附录：退出码

| 退出码 | 含义 |
|--------|------|
| `0` | 成功 |
| `1` | 一般错误（参数错误、记录不存在等） |
| `2` | 数据库错误 |
| `3` | 导入/导出 I/O 错误 |
| `130` | 用户中断（SIGINT，如 Ctrl+C） |
