# dzc CLI 使用文档

`dzc` 是 DezirClip 剪贴板管理器的命令行界面工具。通过终端即可快速查看、搜索、添加、导出剪贴板历史记录，适合与 fzf、shell 脚本或其他自动化工具配合使用。

`dzc` 直接读写桌面端使用的同一个 SQLite 数据库：没有守护进程、没有 IPC、没有令牌，也不会发起任何网络请求。**主程序未运行时 `dzc` 依然可用**。

记录 ID 是整数，`timestamp` 是毫秒级 Unix 时间戳。

## 安装

`dzc` **不单独发布**，它作为 sidecar 随 DezirClip 应用一起安装，位置在主程序二进制旁边。没有独立的下载包，也不在 GitHub Releases 上单独提供。

### Linux

`dzc` 与主程序二进制在同一目录（deb / rpm 安装时位于系统包目录，AppImage 则在镜像挂载目录内）。先定位再加入 `PATH`：

```bash
# 定位随应用安装的 dzc
find /usr/bin /opt -maxdepth 3 -name 'dzc*' -type f 2>/dev/null

# 软链到 PATH 目录
sudo ln -sf "$(command -v dzc || find /usr/bin /opt -maxdepth 3 -name 'dzc*' -type f | head -1)" /usr/local/bin/dzc
```

### Windows

安装后位于 `%LOCALAPPDATA%\DezirClip\dzc.exe`（与 `dezirclip.exe` 同目录）。

当前会话临时生效：

```powershell
$env:Path += ";$env:LOCALAPPDATA\DezirClip"
```

永久写入用户级环境变量：

```powershell
[Environment]::SetEnvironmentVariable(
  "Path",
  [Environment]::GetEnvironmentVariable("Path", "User") + ";$env:LOCALAPPDATA\DezirClip",
  "User"
)
```

### 验证安装

```bash
dzc --version    # 打印版本号
dzc --help       # 打印命令树
```

### Agent Skill

`skills/dzc-cli/` 是一份独立的 Agent 技能说明（随应用一起打包），`install.sh` / `install.ps1` 把它链接到 `~/.claude/skills/dzc-cli`。这两个脚本安装的是**技能文档**，不是 `dzc` 二进制。

## 命令速查表

共 15 个子命令（`tag` 另有 `add` / `remove` / `list` 三个二级子命令）。

| 命令 | 用途 | 常用选项 |
|------|------|----------|
| `list [N]` | 列出最近 N 条（默认 20，`-1` 全部） | `--kind`, `--tag`, `--pinned`, `--json`, `--ids`, `-q` |
| `search <QUERY>` | 按关键词搜索 | `--mode`（contains / fuzzy / regex / fts5）, `--limit`, `--json`, `-q` |
| `get <ID\|latest>` | 打印单条记录 | `--preview`, `--json`, `-q` |
| `stats` | 显示统计摘要 | `--top N`, `--json` |
| `count` | 打印记录总数 | `--json` |
| `add <CONTENT>` | 添加记录（文本 / `-` 标准输入 / `@<路径>` 文件） | `--type` |
| `edit <ID> <CONTENT>` | 替换记录内容并重建预览 | `-q` |
| `delete <ID>` | 删除指定记录 | `-y, --yes`（必填） |
| `clear` | 清空未保护的历史（置顶与带标签的记录保留） | `--yes`（必填）, `-q` |
| `pin <ID>` | 置顶记录 | `-q` |
| `unpin <ID>` | 取消置顶 | `-q` |
| `tag add <ID> <TAG>` | 为记录添加标签 | 无 |
| `tag remove <ID> <TAG>` | 移除记录标签 | 无 |
| `tag list <ID>` | 列出记录的所有标签 | 无 |
| `export <PATH>` | 导出剪贴板历史 | `--encrypted`, `--passphrase`, `-q` |
| `import <PATH>` | 导入历史文件 | `--mode`（merge / replace）, `--passphrase`, `-q` |
| `watch` | 实时监控新记录 | `--pattern`（子串）, `--interval`, `--json`, `-q` |

## dzc list

列出最近的剪贴板历史记录。

### 基本用法

```bash
dzc list          # 默认最近 20 条
dzc list 50       # 最近 50 条
dzc list -- -1    # 全部记录
```

> 数量上限是**位置参数**，不是 `--limit`。因为 `-1` 以 `-` 开头会被当成选项，需要放在 `--` 之后，且所有选项必须写在 `--` 前面（如 `dzc list --ids -- -1`）。

### 过滤选项

```bash
# 只看某个内容类型
dzc list --kind text

# 按标签过滤
dzc list --tag todo

# 只看置顶记录
dzc list --pinned
```

### 输出格式

```bash
# JSON 数组，供脚本解析
dzc list --json

# 只输出 ID，每行一个，配合 fzf 使用
dzc list --ids

# 成功时完全不输出，仅看退出码
dzc list --quiet
```

### 组合过滤

```bash
dzc list --kind text --tag work --json 50
```

## dzc search

在剪贴板历史中搜索关键词。

### 基本用法

```bash
dzc search "TODO"
dzc search "function main"
```

### 搜索模式

```bash
# 包含匹配（默认，大小写不敏感）
dzc search "fix bug" --mode contains

# 模糊匹配（字符按顺序子序列匹配，拼写容错）
dzc search "fuction" --mode fuzzy

# 正则匹配
dzc search "href=.*example\\.com" --mode regex

# FTS5 全文搜索（对大文本更高效）
dzc search "important meeting notes" --mode fts5
```

> 没有 `exact` 模式，合法值只有 `contains`、`fuzzy`、`regex`、`fts5`。

### 限制结果条数

```bash
dzc search "TODO" --limit 10
```

### 搜索并导出

```bash
dzc search "password" --json > matches.json
```

> `search` 没有 `--ids`，需要 ID 时用 `dzc search "..." --json | jq -r '.[].id'`。

## dzc get

打印指定剪贴板记录的完整内容。

### 基本用法

```bash
# 按 ID 查看（ID 为整数）
dzc get 142

# 查看最新一条
dzc get latest
```

### 预览模式

```bash
# 只打印 preview 字段
dzc get 142 --preview
```

### JSON 输出

```bash
dzc get 142 --json
```

### 结合 list 使用

```bash
dzc list --ids | fzf | xargs dzc get
```

## dzc add

手动向剪贴板历史添加一条记录。

### 基本用法

```bash
# 添加纯文本
dzc add "Hello, World!"

# 添加多行文本
dzc add "第一行
第二行
第三行"
```

成功时打印 `Added: id=<ID>`。

### 指定类型

```bash
# HTML 格式
dzc add "<p>你好，<b>世界</b></p>" --type html

# 代码
dzc add "fn main() {}" --type code
```

### 从标准输入读取

```bash
echo "从管道输入" | dzc add -
cat report.txt | dzc add - --type text
```

### 从文件读取

文件路径必须带 `@` 前缀：

```bash
dzc add @/path/to/screenshot.png --type image
```

> `add` 只有 `--type` 一个选项，没有 `--tag` / `--pin`。标签用 `dzc tag add`，置顶用 `dzc pin`。

## dzc edit

替换已有记录的内容，并重建其预览（取内容前 500 个字符）。

```bash
dzc edit 142 "更正后的内容"
dzc edit 142 @/tmp/replacement.txt
```

`CONTENT` 的取值方式与 `add` 一致：纯文本、`-` 标准输入、`@<路径>` 文件。

内容类型、标签和置顶状态保持不变，只有 `content` 与预览被重写。

成功时打印 `Edited: id=<ID>`；`--quiet` 静默。ID 不存在时报 `edit: no entry with id=<ID>`。

## dzc delete

删除指定的剪贴板记录。

### 基本用法

```bash
# 删除单条记录（--yes 必填，删除不可恢复）
dzc delete 142 --yes
```

> `delete` 永远不会弹出确认框：不带 `--yes` 时直接报错退出（退出码 1）。成功时没有输出。

### 批量删除

```bash
# 先获取 ID 列表，再删除
dzc list --ids | fzf -m | xargs -I {} dzc delete {} --yes
```

## dzc clear

清空未保护的历史：既没有置顶、也没有任何标签的记录会被删除，置顶记录和带标签的记录会保留。`--yes` 必填；不带该选项时直接报错退出，不会提示。

```bash
dzc clear --yes
```

成功时打印 `Cleared <N> entries`，其中 N 是实际删除的条数（清空前总数减去清空后总数）；`--quiet` 静默。**删除不可恢复**，执行前请与用户确认。

## dzc pin / unpin

固定重要记录，防止被自动清理。

### 置顶

```bash
dzc pin 142        # 打印 pin: id=142
```

### 取消置顶

```bash
dzc unpin 142      # 打印 unpin: id=142
```

两者都支持 `--quiet` 静默，都没有 `--yes`，也不会弹确认框。ID 不存在时报 `pin: no entry with id=<ID>` / `unpin: no entry with id=<ID>`。

### 查看所有置顶记录

```bash
dzc list --pinned
```

## dzc count

打印记录总数，适合脚本判断。

```bash
dzc count           # 1234
dzc count --json    # {"count":1234}
```

## dzc tag

管理剪贴板记录的标签，方便分类和过滤。ID 为整数。

### 添加标签

```bash
dzc tag add 142 todo
dzc tag add 142 work
```

输出 `tag: added "todo" to id=142`。重复添加同一标签不会报错。

### 移除标签

```bash
dzc tag remove 142 todo
```

输出 `tag: removed "todo" to id=142`。移除不存在的标签视为成功。

### 列出标签

```bash
# 列出某条记录的所有标签
dzc tag list 142

# 列出所有已使用的标签（通过 list + 去重实现）
dzc list --json -- -1 | jq -r '.[].tags[]?' | sort -u
```

无标签时打印 `(no tags)`。

## dzc export / import

导出和导入剪贴板历史，支持加密导出。

### 导出

```bash
# 导出为明文 JSON
dzc export /tmp/backup.json

# 加密导出（路径以 .dzc / .tiez 结尾时自动加密）
dzc export /tmp/backup.dzc --encrypted --passphrase "my-strong-pass"
```

输出 `Exported <N> entries to <PATH> (json|encrypted)`。密码至少 12 个字符，加密输出缺少 `--passphrase` 会直接报错。

### 导入

```bash
# 合并模式：按 ID 覆盖同 ID 记录，其余保留
dzc import /tmp/backup.json --mode merge

# 替换模式：先清空现有历史再导入（谨慎使用）
dzc import /tmp/backup.json --mode replace
```

输出 `Imported <N>/<总数> entries from <PATH> (json|encrypted, mode merge)`。

### 导入加密备份

```bash
dzc import /tmp/backup.dzc --mode merge --passphrase "my-strong-pass"
```

## dzc stats

查看剪贴板使用统计。

```bash
dzc stats           # 人类可读
dzc stats --top 10  # most_used 区块取前 10 条（默认 5，传 0 隐藏该区块）
```

输出示例：

```
Total: 1234 entries

By type:
  html: 50
  image: 84
  text: 1100

Most used (top 3):
  1. [id=42, used=17] git commit -m "fix: ..."
  2. [id=99, used=12] pnpm run build
  3. [id=7, used=9] docker compose up -d
```

### JSON 格式统计

```bash
dzc stats --json
```

```json
{
  "total": 1234,
  "by_type": { "text": 1100, "image": 84, "html": 50 },
  "most_used": [{ "id": 42, "use_count": 17, "preview": "git commit ..." }]
}
```

## dzc watch

实时监控新增的剪贴板记录。

### 基本用法

```bash
dzc watch                    # 监控所有新记录
dzc watch --quiet            # 隐藏启动横幅，条目照常输出
dzc watch --interval 1000    # 轮询间隔（毫秒，默认 500，范围 50–60000）
```

### 按模式过滤

```bash
# 仅监控包含 "TODO" 的记录
dzc watch --pattern "TODO"
```

> **`--pattern` 是大小写不敏感的子串匹配，不是正则。** `"password|secret"` 不会匹配 `password`，它只匹配字面量 `password|secret`。一次运行只能给一个模式；需要多条件时请运行多个 watch，或用 `grep -E` 处理输出流。

### 结合外部通知工具

`watch` 自身没有 `--notify` / `--cmd` 选项，通知由管道另一侧完成：

```bash
# Linux 使用 notify-send
dzc watch --json \
  | while read -r line; do
      notify-send "clipboard" "$(echo "$line" | jq -r '.preview')"
    done
```

```bash
# 直接 grep 输出流做告警
dzc watch --pattern "urgent" | grep --line-buffered -i urgent
```

Ctrl-C 退出时返回退出码 130。

## 常用模式示例

### fzf 集成

通过 `fzf` 交互式选择剪贴板记录，然后查看详情：

```bash
dzc list --ids | fzf --prompt="选择剪贴板记录 > " | xargs dzc get
```

浏览时直接显示内容摘要：

```bash
dzc list --json -- -1 | jq -r '.[] | "\(.id)\t\(.preview)"' \
  | fzf --with-nth=2 --delimiter='\t' | cut -f1 | xargs dzc get
```

### 监控 TODO

在开发过程中监控所有新增的 TODO 项：

```bash
dzc watch --pattern "TODO" --json
```

批量筛选含关键字的记录：

```bash
dzc list --json -- -1 \
  | jq -r '.[] | select(.preview | test("fixme|bug")) | .id' \
  | xargs -I {} dzc get {} --preview
```

### 快速添加

从命令行快速将文本加入剪贴板历史：

```bash
dzc add "pnpm run build"
printf '%s' "$(pwd)" | dzc add -
```

### 导出加密备份

创建加密的剪贴板历史备份：

```bash
dzc export ~/backup.dzc --encrypted --passphrase "$(pass show clipper/backup-pass)"
```

定时任务自动备份（路径为上文的 PATH 配置）：

```bash
0 2 * * * dzc export ~/backup/dezirclip-$(date +\%Y\%m\%d).dzc --encrypted --passphrase "$BACKUP_PASS"
```

### 跨设备同步（占位）

跨设备同步功能在规划中，当前版本的数据迁移需手动导出导入：

```bash
# 设备 A 导出
dzc export /tmp/sync.dzc --encrypted --passphrase "shared-secret"

# 通过 scp / USB 传输 /tmp/sync.dzc 到设备 B

# 设备 B 导入（合并模式）
dzc import /tmp/sync.dzc --mode merge --passphrase "shared-secret"
```

> 注意：自动云同步尚未实现。如需多设备无缝同步，请关注后续版本更新。

## 输出格式

### 人类可读输出

`list` / `search` / `watch` 每条记录一行：`<置顶标记><图标> [类型] <预览>`，多行预览会压成一行。

```bash
$ dzc list 3
📌 📋 [text] git commit -m "fix: ..."
📋 🌐 [html] <b>Hello</b> World
🖼️ [image] screenshot.png
```

### JSON 输出

`--json` 输出结构化数据，`list` / `search` 为数组，`get` / `stats` 为单个对象，`watch --json` 为逐行 NDJSON。

```json
[
  {
    "id": 142,
    "type": "text",
    "content": "Hello World",
    "preview": "Hello World",
    "tags": ["work"],
    "pinned": true,
    "timestamp": 1768461780000
  }
]
```

### ID 列表输出

`--ids` 仅输出 ID（仅 `list` 支持），每行一个：

```bash
$ dzc list --ids
142
377
904
```

### 安静模式

`--quiet` 成功时不输出任何内容，只用退出码判断结果：

```bash
$ dzc list --quiet
$ echo $?
0
```

## 故障排除

### 命令无响应或卡住

**症状：** `dzc watch` 长时间无输出。

**原因：** 这是预期行为——`watch` 是长驻进程，只有新记录出现时才打印。

**解决：** 用 Ctrl-C 结束（退出码 130）。若 `dzc list` 报 `database is locked`，说明另一个 `dzc` 或桌面端正在写入同一个数据库文件，等待其完成即可；`dzc` 不需要主程序运行。

### 导入时提示格式错误

**症状：** `dzc import backup.json` 报 "parse json" 失败。

**原因：** 导入文件不是合法的 dzc 导出格式，或文件已损坏。

**解决：**

```bash
# 验证 JSON 格式
jq . backup.json

# 确认文件由 dzc export 生成
head -5 backup.json
# 应看到 {"version": "dezirclip-export-v1", "exported_at": ...}
```

### 搜索无结果但内容确实存在

**症状：** `dzc search "foo"` 返回空。

**原因：** 模式不匹配，或目标条目的 `content` 里没有该关键词（例如图片条目存的是路径而非正文）。

**解决：**

```bash
# 尝试模糊模式
dzc search "foo" --mode fuzzy

# 确认记录类型
dzc list --kind text --json | jq '.[] | select(.content | test("foo"))'

# 如为图片，确认条目存在并查看预览
dzc list --kind image
```

### 记录 ID 不存在

**症状：** `get: no entry for id '999'`。

**原因：** 该 ID 已被删除或从未存在。

**解决：**

```bash
dzc list --ids -- -1 | tail -20
```

### 导出加密文件无法导入

**症状：** 导入时报 `import: decrypt failed: ...`。

**原因：** 密码错误或文件损坏。

**解决：**

```bash
# 确认密码无误
dzc export /tmp/test.dzc --encrypted --passphrase "correct-passphrase"
dzc import /tmp/test.dzc --passphrase "correct-passphrase"

# 如使用 scp 传输，确认文件完整
md5sum /tmp/backup.dzc  # 与源端对比
```

## 附录：环境变量

`dzc` 只读取以下两个变量：

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `DEZIRCLIP_DB_PATH` | 指定数据库文件路径 | 平台默认路径 |
| `TIEZ_DB_PATH` | 旧版别名，仅在上一项未设置时生效 | 平台默认路径 |

默认路径：

- Windows：`%APPDATA%\io.github.qiyuanhuakai.dezirclip\clipboard.db`（若不存在，回退到旧版目录 `%APPDATA%\com.tiez.clipboard\clipboard.db`）
- Linux：`$XDG_DATA_HOME/io.github.qiyuanhuakai.dezirclip/clipboard.db`，未设置 `XDG_DATA_HOME` 时用 `$HOME/.local/share/io.github.qiyuanhuakai.dezirclip/clipboard.db`

示例：

```bash
export DEZIRCLIP_DB_PATH=/mnt/backup/dezirclip.db
dzc list
```

## 附录：退出码

| 退出码 | 含义 |
|--------|------|
| `0` | 成功 |
| `1` | 运行错误（缺少 `--yes`、记录不存在、数据库或 I/O 失败、密码错误） |
| `2` | 参数解析错误（未知选项、未知子命令、非法取值，例如 `--mode exact`） |
| `130` | 用户中断（SIGINT，如 `watch` 下的 Ctrl-C） |

不返回 `3` 或其他退出码。
