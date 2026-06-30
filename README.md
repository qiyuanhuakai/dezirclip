# DezirClip

DezirClip is a local clipboard workspace for Windows and Linux, built with Tauri 2, Rust, React, and TypeScript.

It focuses on desktop-local clipboard history, quick paste, region screenshots, OCR-assisted image search, text transforms, tags, backup import/export, and the `dzc` command-line tool.

## English

### Highlights

- Windows and Linux support
- Local SQLite clipboard history
- Quick-paste overlay and multiple hotkeys
- Region screenshot capture
- OCR text storage and search for copied images
- Rich text, image, file, URL, code, and tag-aware clipboard entries
- Six bundled themes with runtime theme switching
- `dzc` CLI for add/list/search/get/tag/import/export workflows
- Agent Skill under [`skills/dzc-cli`](./skills/dzc-cli/)

### Development

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
cd ../dzc-standalone && cargo test
```

Build artifacts are written under `src-tauri/target/release/bundle/`.

### Linux dependencies

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y \
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
  xdotool
```

### CLI and Agent Skill

The CLI binary is named `dzc`.

```bash
dzc --help
```

The Agent Skill is available in [`skills/dzc-cli`](./skills/dzc-cli/). Copy or link that directory into your Agent skills directory.

### Release

CI runs on pushes and pull requests to `master`. A GitHub Release is created automatically when a `v*` tag is pushed.

Example:

```bash
git tag v0.0.1
git push origin v0.0.1
```

### License and attribution

DezirClip is licensed under GPL-3.0.

This project is derived from [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard), which is also GPL-3.0 licensed. Original copyright and license notices are retained where applicable.

## 中文

DezirClip 是一个面向 Windows 和 Linux 的本地剪贴板工作台，使用 Tauri 2、Rust、React 和 TypeScript 构建。

它关注桌面本地的剪贴板历史、快速粘贴、区域截图、基于 OCR 的图片搜索、文本转换、标签、备份导入导出，以及 `dzc` 命令行工具。

### 功能亮点

- 支持 Windows 和 Linux
- 使用本地 SQLite 保存剪贴板历史
- 快速粘贴浮层和多组快捷键
- 区域截图捕获
- 图片 OCR 文本保存与搜索
- 支持富文本、图片、文件、URL、代码和标签感知的剪贴板条目
- 内置六套主题，并支持运行时切换
- `dzc` CLI 支持 add/list/search/get/tag/import/export 工作流
- Agent Skill 位于 [`skills/dzc-cli`](./skills/dzc-cli/)

### 开发

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
cd ../dzc-standalone && cargo test
```

构建产物位于 `src-tauri/target/release/bundle/`。

### Linux 依赖

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y \
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
  xdotool
```

### CLI 和 Agent Skill

CLI 可执行文件名为 `dzc`。

```bash
dzc --help
```

Agent Skill 位于 [`skills/dzc-cli`](./skills/dzc-cli/)。将该目录复制或链接到你的 Agent skills 目录即可使用。

### 发布

CI 会在推送到 `master` 或向 `master` 发起 pull request 时运行。推送 `v*` tag 时会自动创建 GitHub Release。

示例：

```bash
git tag v0.0.1
git push origin v0.0.1
```

### 许可证与来源

DezirClip 使用 GPL-3.0 许可证。

本项目派生自同样使用 GPL-3.0 许可证的 [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard)。适用的原始版权和许可证声明会继续保留。
