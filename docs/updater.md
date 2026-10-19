# 签名更新与发布通道

桌面更新使用官方 Tauri 2 Updater。v0.2 的首批原位更新制品仅包含 Windows NSIS 和 Linux AppImage，支持 x86_64 与 aarch64。`dzc-core`、独立 CLI、MCP 和捕获服务不因此联网，也不增加后台进程。P1-A 提供后端与发布门槛，P1-C 在“常规设置 → 软件更新”接入自动检查偏好、通道、手动检查及确认安装界面。

## 设置界面与自动检查

新增 `app.update_auto_check`（字符串布尔值，默认 `"false"`）和 `app.update_channel`（`"stable"` / `"preview"`，默认 `"stable"`），保留全部既有设置键。偏好经后端确认保存后才更新控件；保存失败显示本地化错误并保持原值。

自动检查须由用户主动启用。主窗口设置加载完成后非阻塞初始化，并在 15 秒后按需检查；每个通道最多 24 小时自动尝试一次，不启动周期轮询或新常驻进程。尝试时间在调用 Rust 之前写入 `localStorage`，失败请求也计入间隔，避免反复启动、React StrictMode 或空闲销毁/重建 WebView 导致重复自动请求。保存间隔失败时跳过自动联网，用户仍可手动检查。长时间保持同一个 WebView 的用户可使用“立即检查”；不会在后台每隔一段时间自动唤醒网络。

手动检查忽略自动间隔，并使本通道的下一次自动检查顺延 24 小时。同一 WebView 的请求由 feature 内共享客户端去重，跨 WebView 重建的并发由 P1-A 后端互斥继续约束；后端报告已有操作时，界面提示稍后手动重试。界面内存中的检查结果在 WebView 重建后会丢弃，安装前可重新检查，不持久化可执行更新信息或旧 opaque ID。

设置页显示当前/可用版本、检查状态、未发布索引、纯文本版本说明、下载进度和已验签状态。版本说明不执行 HTML 或 Markdown。点击“安装更新…”先显示版本和退出/重启确认，只有再点“下载、安装并重启”才调用安装命令；取消、查看说明或启用自动检查均不下载制品、不安装。失败信息使用本地化的有限错误类别，不向界面直接回显服务器响应。

不支持原位更新时隐藏安装按钮并说明只读 AppImage、包管理、便携版或未打包形式的处理方式。“打开此版本发布页”定位已发现版本，由用户按原安装方式选择对应包；不会向 deb/rpm/Flatpak 用户下载并覆盖 AppImage。安装失败后丢弃旧 ID，需重新检查。进度只通知设置模块订阅者，不推动整个 App 随每个下载分块重绘；普通下载进度通知限为约每 100ms 一次，状态变化即时通知。更新客户端单独分块，启动初始化无需加载全部设置 UI。

## 安装支持范围

| 运行形式 | 应用内行为 |
| --- | --- |
| Windows NSIS | 官方 Updater 下载、验签后运行 passive 安装器；程序退出，安装器允许重启 |
| 可写 AppImage | 确认包文件及同目录可写后替换，并重启 |
| 只读 AppImage | 只检查版本，提示移到可写位置或打开发布页下载 |
| deb / rpm / Flatpak | 只检查版本，使用相应包管理方式或下载匹配软件包 |
| Portable / 未打包程序 | 只检查版本，提示手动替换或安装受支持的包 |

不使用 AppImage 覆盖 deb/rpm/Flatpak，不修改独立 CLI 的 PATH。GUI 与随包 `dzc` 由同一次版本构建和安装共同替换。旧版没有 Updater，需要先手动覆盖安装一次；升级安全还依赖 P1-B 的 NSIS 修正。旧卸载器的历史行为及迁移限制见该 PR 的安装文档。

可写检查只是安装前检查，文件权限、空间或挂载状态仍可能在下载后改变；安装失败会返回明确错误。实际 Windows 安装、AppImage 替换、数据保留和错误签名安装拒绝仍需要打包后的端到端验收，编译通过不能代替这些验收。

## 两个固定索引

- Stable：`https://github.com/qiyuanhuakai/dezirclip/releases/download/updater-channels/stable.json`
- Preview：`https://github.com/qiyuanhuakai/dezirclip/releases/download/updater-channels/preview.json`

Stable 只读取 stable.json，排除全部预发布。Preview 分别读取两个索引，选取 SemVer precedence 较新的版本，因此也可以收到比预览版新的正式版。相同版本、仅 build metadata 不同及较旧版本均不安装。由 Preview 改为 Stable 不会降级。

每次正式发布只推进 stable.json；每次预发布只推进 preview.json。首次发布时，另一索引可以尚未存在：HTTP 404 作为 `unpublishedChannels` 返回；两者均不存在时返回 `update_channel_not_published`。离线、非 404 HTTP 错误、无效 JSON、错误平台/签名元数据会返回失败，不显示成“已经是最新版本”。

前端无法指定 endpoint、公钥或制品 URL。索引下载有 15 秒超时和 256 KiB 上限，制品下载有 10 分钟超时。制品原始 URL 必须指向本仓库 `releases/download/v<version>/` 下与当前 OS/架构精确匹配的文件，且使用 HTTPS。GitHub 资产重定向仅允许 HTTPS 的 github.com、release-assets.githubusercontent.com、objects.githubusercontent.com 和 github-releases.githubusercontent.com。GitHub 更改资产域名后须更新并审查此名单。

## 后端接口

以下 Tauri 命令通过 Rust 封装官方插件，不开放插件的通用前端检查/下载权限。

| 命令 | 参数 | 返回/行为 |
| --- | --- | --- |
| `get_update_support` | 无 | `installation`, `canInstall`, `reason`, `releasesUrl` |
| `check_for_update` | `channel: "stable" / "preview"` | `currentVersion`, `channel`, `support`, `available`, `unpublishedChannels` |
| `install_update` | `updateId`, `onProgress: Channel` | 下载、官方验签、安装、重启；必须显式调用 |

`available` 为 `null` 或 `{ id, version, notes, releaseUrl }`。安装只接受最近一次成功检查产生的 opaque ID；重新检查立即使旧 ID 失效。检查或安装期间的并发请求返回 `update_busy`。更新描述是外部文本，UI 按纯文本渲染。

进度事件依次为 `{ event: "downloading", downloaded, total }`、`{ event: "verified" }`、`{ event: "installing" }`。下载完成不等于验签通过；只有官方 `download` 验签成功后发出 verified，官方 `install` 再次验证包。Windows 安装成功启动后官方插件直接退出当前进程，命令通常不会再向前端返回。Linux 安装成功后主动重启应用。

`reason` 支持 `nsis_in_place`、`appimage_in_place`、`appimage_not_writable`、`use_package_manager`、`replace_portable_manually`、`install_supported_bundle`。错误代码包括 `update_signing_not_configured`、`update_channel_not_published`、`update_check_failed`、`update_server_failed`、`update_invalid_manifest`、`update_invalid_stable_manifest`、`update_untrusted_download`、`update_invalid_signature`、`update_signature_version_mismatch`、`update_download_or_signature_failed`、`update_install_failed`、`update_installation_unsupported`、`update_check_required`、`update_busy`。不会返回剪贴板内容或签名私钥。

## 签名与发布门槛

公钥内置 `src-tauri/tauri.conf.json`。私钥及其口令仅通过受保护的 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` Actions Secrets 供打包任务使用，不写入仓库或更新索引。缺少私钥、公钥无效或签名配置不完整时发布预检失败。丢失签名密钥后不能向现有安装推送新签名更新，须由维护者独立备份密钥。

使用 CLI 2.12.3 生成带 authenticated `version:` trusted comment 的官方签名，Updater 2.14 设置 `requireSignedVersion: true`。签名覆盖制品及版本；普通 SHA-256 文件只能用于传输校验，不能替代签名。没有版本的旧签名、元数据伪造版本配旧包、制品正文或签名被篡改时均拒绝安装。

现有 Release 工作流按以下顺序运行：

1. 检查公钥、Secrets、固定 endpoint 与版本签名配置，并运行发布逻辑回归。
2. 创建 draft release；四平台桌面包、四平台独立 CLI 和 Flatpak 均上传至 draft。
3. 所有构建任务成功后，下载四个 NSIS/AppImage 制品及其 `.sig`，验证存在性、长度、Ed25519/BLAKE2b 签名、签名版本及文件名；发布前再次确认资产身份未变化。
4. 发布版本 release，确认其已公开。
5. 比较当前通道版本，仅在严格较新时推进该发布对应的一个索引；先上传版本命名的临时资产并回读核对内容，再重命名活动索引。旧活动索引保留为 `*-previous-<asset-id>.json` 供恢复。

整个 release 工作流串行，禁止取消正在运行的发布，避免 Stable/Preview 互相覆盖或旧版本后完成导致回退。签名缺失、缺包、验签失败及构建失败均保持 draft，不推进通道。已公开且有完整已验签制品的旧版本允许保留发布，但不覆盖较新通道。

Flatpak 构建用 deb 暂存根目录时显式关闭 updater artifact 生成；Flatpak 走包管理方式，不获取桌面更新签名私钥。正式桌面发布任务的 deb/rpm 可能额外产生签名，但它们不进入应用内更新索引。

GitHub Release 资产重命名不提供事务：替换活动索引时可能短暂出现 404。脚本先保留旧资产，重命名失败会检查操作是否其实成功，并尝试恢复旧名称；断网或 GitHub 故障可能需要维护者恢复活动资产名。这影响索引可用性，不绕过客户端签名/版本检查。首次通道 release 也先建 draft，索引上传核验后才公开。遗留 draft channel release 会中止自动操作，维护者应检查后恢复，不自动猜测未完成状态。

如果版本已公开但通道推广失败，可以在相同可信 tag 的 checkout 内重新运行 `node scripts/updater-release.mjs publish v<version>`；脚本重新验证公开制品并拒绝降级。普通发布工作流拒绝重建已公开的同 tag，避免替换已安装版本的制品。

## 验证命令与边界

```powershell
node --test scripts/tests/updater-release.node.mjs
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib domain::update --locked
cargo check --manifest-path src-tauri/Cargo.toml --bin dezirclip --locked
```

签名回归使用 Tauri 官方公开测试向量，覆盖正文篡改、trusted comment 篡改、版本重放及缺版本。纯逻辑回归覆盖 SemVer 数字预发布序号、正式版提升、build metadata、禁止回退、缺制品及文件名绑定。没有新增前端测试框架或 Cargo dev-dependencies。

依赖升级使 Tauri 使用 Windows 0.62 / WebView2 0.39，直接依赖同步对齐；前端 `@tauri-apps/api` 同步到 2.12，与 Rust 的同一 major/minor 匹配。Windows OCR 的阻塞等待改用 windows-future 0.3 的 `join()`。既有 Win32、内存管理和 OCR 行为仍需要正式包回归。

参考：[Tauri Updater](https://v2.tauri.app/plugin/updater/)、[Updater 2.14 API](https://docs.rs/tauri-plugin-updater/2.14.0/tauri_plugin_updater/)、[Tauri CLI 2.12.3](https://github.com/tauri-apps/tauri/releases/tag/tauri-cli-v2.12.3)、[Minisign 格式](https://jedisct1.github.io/minisign/)、[windows-future 0.3.2](https://docs.rs/windows-future/0.3.2/windows_future/struct.IAsyncOperation.html)。
