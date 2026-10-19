# DezirClip Linux 安装指南

## 支持范围

当前支持 Linux X11 桌面，发布构建包含 x86_64 与 aarch64；下载与机器架构匹配的安装包。系统需具备可用的 X11 会话和 WebKitGTK 4.1。完整 Wayland 支持暂缓，XWayland 下可能运行的部分功能不代表后台监听、全局快捷键、焦点恢复和自动粘贴都已适配。

历史管理、原生剪贴板读写与终端集成是不同能力：无图形会话时 `dzc list/search/get` 仍可访问数据库，`dzc clipboard read/write` 则需要可访问的 X11 服务。通过 SSH 使用时，它操作的是能访问的远程图形会话。

## 安装依赖

以下为源码构建依赖，与 [README](../README.md#linux-dependencies) 保持一致。普通 deb/rpm 安装会由包管理器解析打包声明的运行依赖。Tauri 2 使用 WebKitGTK **4.1**；旧指南中的 `libwebkit2gtk-4.0-dev` 不适用。发行版基础包名参考 [Tauri 2 官方前置依赖](https://v2.tauri.app/start/prerequisites/#linux)。

### Ubuntu / Debian

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

### 其他发行版

先按上述 Tauri 官方指南安装本发行版的基础开发依赖，再安装截图、X11 与 OCR 所需的对应包。重要包名区别：

| 发行版 | WebKitGTK 4.1 开发包 | 应用指示器相关包 |
| --- | --- | --- |
| Arch Linux | `webkit2gtk-4.1` | `libappindicator-gtk3` |
| Fedora | `webkit2gtk4.1-devel` | `libappindicator-gtk3-devel` |
| openSUSE | 官方指南列为 `webkit2gtk3-devel`，需确认提供 `webkit2gtk-4.1` 的 pkg-config 文件 | `libappindicator3-1` |

构建前可确认 WebKit 开发文件：

```bash
pkg-config --modversion webkit2gtk-4.1
```

`tesseract` 用于 Linux OCR，可按需求安装语言数据。`xdotool` 用于自动发送粘贴按键；原生剪贴板读写与 `dzc copy` 不需要它，也不依赖 `xclip`、`xsel` 或 `wl-copy`。Linux 托盘使用 StatusNotifierItem/D-Bus，桌面环境需要能显示对应托盘图标。

## 从源码构建

准备稳定版 Rust、Node.js 22.12+ 和 `package.json` 固定的 pnpm 11.0.9。当前 Vite 7 的 Node 范围为 `^20.19.0 || >=22.12.0`，Node 18 或较早的 Node 20 不满足要求；参见 [Vite 环境要求](https://vite.dev/guide/#scaffolding-your-first-vite-project)。项目已包含 Tauri CLI，无需另装全局 CLI。

```bash
git clone https://github.com/qiyuanhuakai/dezirclip.git
cd dezirclip
pnpm install
pnpm run tauri:dev
```

需要暂时关闭窗口追踪、边缘吸附和鼠标快捷键服务来排查启动问题时：

```bash
pnpm run tauri:dev:safe
```

正式构建会先构建前端与 `dzc` sidecar，再打包桌面应用：

```bash
pnpm run tauri:build
```

产物位于 `target/release/bundle/`，包含 deb、AppImage、rpm。不要分发缺少 `dist/web` 的手工桌面构建。只需 CLI 时可运行 `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin dzc --release`，输出 `target/release/dzc`，无需 GTK/WebKit/PipeWire 开发依赖。跨发行版分发使用 `scripts/build-standalone-dzc.py` 的 Ubuntu 22.04 Docker 构建路径，详见 [独立 CLI 说明](./dzc-standalone.md)。

## 安装发布包

从 [GitHub Releases](https://github.com/qiyuanhuakai/dezirclip/releases) 下载对应架构的文件。以下 `VERSION_ARCH` 是文件名占位，请替换为实际下载的文件名：

```bash
# AppImage
chmod +x ./DezirClip_VERSION_ARCH.AppImage
./DezirClip_VERSION_ARCH.AppImage

# Debian / Ubuntu
sudo apt install ./dezirclip_VERSION_ARCH.deb

# Fedora
sudo dnf install ./dezirclip_VERSION_ARCH.rpm
```

openSUSE 使用其包管理器安装 rpm。AppImage 运行依赖还取决于发行版的 FUSE 配置；若挂载失败，先按发行版说明排查，或使用 deb/rpm。`dzc` 随应用安装，位置随打包形式变化；在设置中查看 CLI 路径后加入 `PATH`，详见 [CLI 文档](./cli.md)。

### Flatpak

新构建的 Flatpak 内置 `xdotool`、Tesseract 5.5.3、Leptonica 和 `eng` / `chi_sim` / `osd` 模型，不需要宿主机安装 OCR 或粘贴工具。已发布的 alpha.1/alpha.2 附件没有这些工具；此变化适用于新 CI 制品和后续发布。

```bash
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./DezirClip-VERSION-linux-ARCH.flatpak
flatpak run io.github.qiyuanhuakai.dezirclip
```

自动粘贴仍需要 X11 会话、正确的窗口焦点和支持相应快捷键的目标应用。内置工具不会授予 Wayland 输入模拟权限，也不代表完整 Wayland 支持。OCR 在沙箱内本地执行，默认使用英文和简体中文；可用 `DEZIRCLIP_OCR_LANG` 选择已安装的语言。

源码构建与验证：安装 `org.gnome.Platform//50`、`org.gnome.Sdk//50`、`flatpak-builder`、Xvfb 和 X11 开发库后，运行 `pnpm run tauri:build`、`pnpm run flatpak:prepare`、`pnpm run flatpak:build`，再运行 `python3 scripts/verify-flatpak.py flatpak/build`。验证使用最终 Platform 运行时识别中英文固定图片，并向隔离的 X11 窗口发送 Ctrl+V、检查剪贴板文本往返；不安装应用、不接触用户桌面的焦点或剪贴板。CI 和发布复用同一构建及验证流程。

## 剪贴板捕获与内容保真

X11 优先订阅 XFixes 的剪贴板所有者变化，纯图片复制同样会触发读取。XFixes 不可用或连接失败时，回退到 100 ms 轮询；指纹包含文本、文件 URI、图片尺寸和像素，不再只检查文本。轮询仍可能漏掉两次采样间已被覆盖的短暂内容，快速连续复制也会受到来源应用和剪贴板可读取时机影响。

文本正文保留首尾空白和原始 CRLF/LF，预览、分类和搜索规范化单独进行。用 `dzc get ID --raw` 可无额外换行地读取存储正文；图片使用 `--mime image/png`。旧版本已修剪的内容无法从历史中恢复。

设置中的“不记录这些应用”对应新键 `app.excluded_source_apps`，默认空。每行填写应用名、可执行文件名或完整路径，忽略大小写并完整匹配，可省略 `.exe`；离开输入框后生效。它仅阻止后续自动捕获，不删除旧记录。来源识别优先读取剪贴板所有者的 PID，缺少时回退前台应用；权限不足或缺少 PID 时不能保证准确识别，因此不能把来源排除当作绝对隐私边界。

`dzc clipboard write` 和 `dzc copy ID` 在 X11 下启动轻量后台进程持有 selection。主 CLI 返回后内容仍可粘贴；被其他应用替换时持有进程退出，不要求 GUI 保持运行。

## 排错

### 启动或链接失败

检查运行时依赖和图形会话：

```bash
ldd target/release/dezirclip | grep 'not found'
printf '%s\n' "$XDG_SESSION_TYPE" "$DISPLAY"
pkg-config --modversion webkit2gtk-4.1
```

在实际 X11 桌面终端运行；只设置 `DISPLAY` 并不能授予另一个用户或容器访问 X11 的权限。

### 复制成功但自动粘贴失败

先用 `dzc clipboard read` 确认内容，再手动粘贴到目标应用。自动按键需要 `xdotool`、正确焦点及目标应用支持的 Ctrl+V/Shift+Insert。`dzc copy` 本身只复制，不模拟按键。

### 没有捕获新记录

确认未暂停监听，且来源应用不在排除列表。日志中可看到 XFixes 启动或轮询回退信息。可单独禁用 XFixes 以检查回退路径：

```bash
DEZIRCLIP_DISABLE_LINUX_SERVICES=clipboard_xfixes pnpm run tauri:dev
```

`DEZIRCLIP_DISABLE_LINUX_SERVICES=clipboard_listener` 会完全关闭剪贴板监听；变量还支持逗号分隔的 `window_tracker`、`edge_docking`、`mouse_hotkey` 和 `all`。旧名称 `TIEZ_DISABLE_LINUX_SERVICES` 仍兼容，优先使用新名称。

监听实现位于 `src-tauri/src/infrastructure/linux_api/clipboard_listener.rs`，原生剪贴板实现与来源识别分别位于同目录的 `clipboard.rs`、`window_tracker.rs`。完整接口与可选终端工具见 [CLI 文档](./cli.md)和 [集成指南](../integrations/README.md)。
