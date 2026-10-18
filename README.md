# DezirClip

DezirClip is a local clipboard workspace for Windows and Linux X11, built with Tauri 2, Rust, React, and TypeScript.

It focuses on desktop-local clipboard history, quick paste, region screenshots, OCR-assisted image search, text transforms, tags, backup import/export, and the `dzc` command-line tool.

## English

### Highlights

- Windows and Linux X11 support
- Local SQLite clipboard history
- Text fidelity: preserve leading/trailing whitespace and original line endings; normalize previews and search separately
- X11 clipboard-owner notifications via XFixes, with image-aware polling when unavailable
- Exclude source applications from future clipboard captures
- Quick-paste overlay and multiple hotkeys
- Region screenshot capture
- OCR text storage and search for copied images
- Rich text, image, file, URL, code, and tag-aware clipboard entries
- Six bundled themes with runtime theme switching
- Standalone `dzc` CLI with JSON/TSV, pagination, raw/MIME payloads, and native system clipboard read/write
- Agent Skill under [`skills/dzc-cli`](./skills/dzc-cli/)

### Product scope and integrations

The core product is desktop-local clipboard history on Windows and Linux X11. The GUI and CLI share Rust storage code. `dzc` can access history while the GUI is closed; native clipboard commands need an accessible graphical session. Full Wayland support is deferred until its clipboard, shortcut, focus, and paste behavior can be tested. Cloud sync and networking features are outside the product scope.

[Optional TUI, fzf/dmenu/rofi pickers, shell insertion, and read-only MCP adapters](./integrations/README.md) run separately through `dzc`. They do not add dependencies to the desktop application. The [CLI reference](./docs/cli.md) documents the public interface; older roadmap entries are historical planning, not a list of missing features.

In **Settings → Clipboard**, “Do not record these apps” accepts one application name, executable name, or full path per line. Matches ignore case, require a whole name/path, and allow omitting `.exe`. The new `app.excluded_source_apps` setting defaults to empty and applies on leaving the field. It blocks future captures before storage; it does not remove existing history. Source detection is best effort: X11 uses the clipboard owner's PID when available and otherwise the foreground application; unknown sources cannot be reliably excluded.

### Development

Use Node.js 22.12+ (the installed Vite 7 requires `^20.19.0 || >=22.12.0`), pnpm 11.0.9 as pinned in `package.json`, and stable Rust. Windows also needs the C++ build tools and WebView2; Linux needs the packages below. See the official [Vite requirements](https://vite.dev/guide/#scaffolding-your-first-vite-project) and [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
pnpm install
pnpm run tauri:dev
pnpm run tauri:build
```

Useful checks:

```bash
pnpm run test:unit
pnpm run build
pnpm run i18n:check
cd src-tauri && cargo test
```

Build artifacts are written under `src-tauri/target/release/bundle/`.

### Linux dependencies

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libxdo-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  pkg-config \
  libclang-dev \
  libpipewire-0.3-dev \
  libgbm-dev \
  libx11-dev \
  libxi-dev \
  libxrandr-dev \
  libxtst-dev \
  tesseract-ocr \
  tesseract-ocr-eng \
  tesseract-ocr-chi-sim \
  xdotool
```

See the [Linux installation guide](./docs/LINUX_INSTALL.md) for packaging and troubleshooting. Clipboard read/write uses native APIs and requires no `xclip` or `xsel`; automatic paste keystrokes still use `xdotool`.

### CLI and Agent Skill

The CLI binary is named `dzc`.

```bash
dzc --help
dzc list --metadata --json --exclude-sensitive
dzc clipboard read
dzc copy 142 --quiet
```

The Agent Skill is available in [`skills/dzc-cli`](./skills/dzc-cli/). Copy or link that directory into your Agent skills directory.

### Release

The existing workflows run checks on pushes to `master` and on pull requests. A GitHub Release is created automatically when a `v*` tag is pushed.

Example:

```bash
git tag v0.0.1
git push origin v0.0.1
```

### License and attribution

DezirClip is licensed under GPL-3.0.

This project is derived from [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard), which is also GPL-3.0 licensed. Original copyright and license notices are retained where applicable.

## 中文

DezirClip 是一个面向 Windows 和 Linux X11 的本地剪贴板工作台，使用 Tauri 2、Rust、React 和 TypeScript 构建。

它关注桌面本地的剪贴板历史、快速粘贴、区域截图、基于 OCR 的图片搜索、文本转换、标签、备份导入导出，以及 `dzc` 命令行工具。

### 功能亮点

- 支持 Windows 和 Linux X11
- 使用本地 SQLite 保存剪贴板历史
- 原样保留文本首尾空白及换行符，预览和搜索单独规范化
- X11 优先通过 XFixes 监听剪贴板所有者变化，不可用时轮询文本、文件和图片
- 按来源应用排除后续剪贴板捕获
- 快速粘贴浮层和多组快捷键
- 区域截图捕获
- 图片 OCR 文本保存与搜索
- 支持富文本、图片、文件、URL、代码和标签感知的剪贴板条目
- 内置六套主题，并支持运行时切换
- 独立 `dzc` CLI 支持 JSON/TSV、分页、原始内容和 MIME 输出，以及原生系统剪贴板读写
- Agent Skill 位于 [`skills/dzc-cli`](./skills/dzc-cli/)

### 产品边界和扩展入口

核心是 Windows 和 Linux X11 上的本地剪贴板历史。GUI 与 CLI 共用 Rust 存储代码；GUI 关闭时 `dzc` 仍能操作历史，系统剪贴板命令则需要可访问的图形会话。Wayland 完整支持暂缓，待剪贴板、快捷键、焦点和粘贴行为具备可复现测试环境后推进。云同步和网络功能不在产品范围内。

[可选 TUI、fzf/dmenu/rofi 选择器、Shell 输入行插入和只读 MCP 适配器](./integrations/README.md) 通过 `dzc` 独立运行，不增加桌面端依赖。[CLI 文档](./docs/cli.md) 记录公共接口；旧 roadmap 保留为历史规划，不代表当前功能缺口。

在**设置 → 剪贴板设置**的“不记录这些应用”中，每行填写一个应用名、可执行文件名或完整路径。忽略大小写，按完整名称或路径匹配，可省略 `.exe`。新设置键 `app.excluded_source_apps` 默认为空，离开输入框后生效，在存储前拦截后续捕获，不删除已有历史。来源识别存在限制：X11 优先读取剪贴板所有者的 PID，缺少时回退到前台应用；无法识别的来源无法可靠排除。

### 开发

使用 Node.js 22.12+（当前 Vite 7 要求 `^20.19.0 || >=22.12.0`）、`package.json` 固定的 pnpm 11.0.9 和稳定版 Rust。Windows 还需 C++ 构建工具与 WebView2；Linux 依赖见下文。参见 [Vite 环境要求](https://vite.dev/guide/#scaffolding-your-first-vite-project)和 [Tauri 2 前置依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install
pnpm run tauri:dev
pnpm run tauri:build
```

常用检查：

```bash
pnpm run test:unit
pnpm run build
pnpm run i18n:check
cd src-tauri && cargo test
```

构建产物位于 `src-tauri/target/release/bundle/`。

### Linux 依赖

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libxdo-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  pkg-config \
  libclang-dev \
  libpipewire-0.3-dev \
  libgbm-dev \
  libx11-dev \
  libxi-dev \
  libxrandr-dev \
  libxtst-dev \
  tesseract-ocr \
  tesseract-ocr-eng \
  tesseract-ocr-chi-sim \
  xdotool
```

安装及排错见 [Linux 安装指南](./docs/LINUX_INSTALL.md)。剪贴板读写使用原生 API，无需 `xclip` 或 `xsel`；自动发送粘贴按键仍需要 `xdotool`。

### CLI 和 Agent Skill

CLI 可执行文件名为 `dzc`。

```bash
dzc --help
dzc list --metadata --json --exclude-sensitive
dzc clipboard read
dzc copy 142 --quiet
```

Agent Skill 位于 [`skills/dzc-cli`](./skills/dzc-cli/)。将该目录复制或链接到你的 Agent skills 目录即可使用。

### 发布

现有工作流会在推送到 `master` 和发起 pull request 时运行检查。推送 `v*` tag 时会自动创建 GitHub Release。

示例：

```bash
git tag v0.0.1
git push origin v0.0.1
```

### 许可证与来源

DezirClip 使用 GPL-3.0 许可证。

本项目派生自同样使用 GPL-3.0 许可证的 [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard)。适用的原始版权和许可证声明会继续保留。
