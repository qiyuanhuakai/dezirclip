# 隔离 AppImage 升级验收

`../test-native-appimage.py` 在 Linux 的临时目录中复制旧包和候选包，运行官方 updater 安装，再启动实际候选 AppImage 的 WebView 验证历史记录与安装能力。它是验收工具，不参与产品构建。

此 helper 有独立 `[workspace]` 和锁文件，固定 `tauri 2.12.4` 的 `test` feature、`tauri-plugin-updater 2.14.0`；没有 dev-dependencies，不改变根 workspace 或产品依赖。helper 使用官方 `restore_update`、`verify`、`install`，传入内存字节；网络客户端一旦被使用就失败。`restore_update` 本身要求签名绑定版本，配置也显式启用 `requireSignedVersion`。

## 运行

需要 Linux、Python 3、Rust，以及可用的 bubblewrap 用户命名空间、Xvfb、`dbus-run-session` 和匹配本机 WebKitGTK 的 `WebKitWebDriver`。WSL 可以执行这套隔离检查；工具不会连接宿主图形会话。依赖缓存准备好后，在仓库根目录运行：

```sh
cargo build --offline --locked \
  --manifest-path scripts/native-updater-fixture/Cargo.toml \
  --target-dir /tmp/dezirclip-v02-native-target --jobs 2

python3 scripts/test-native-appimage.py \
  --old-appimage /path/to/DezirClip_0.1.20_amd64.AppImage \
  --candidate /path/to/DezirClip_0.2.0-alpha.2_amd64.AppImage \
  --public-key /path/to/test-updater.key.pub \
  --helper /tmp/dezirclip-v02-native-target/debug/dezirclip-native-updater-acceptance \
  --webkit-driver /path/to/WebKitWebDriver \
  --source-revision "$(git rev-parse HEAD)"
```

候选旁必须有同名 `.AppImage.sig`。仅传入验签公钥；工具不生成、读取或复制私钥。旧 0.1.20 包固定 SHA256 为 `da4abe1af820226ccf0cb8c40c8d7512b5ae3d7de89babcd1acecfd1ece9f0f0`；候选文件名和实际 CLI 版本固定为 `0.2.0-alpha.2`。`--source-revision` 是可选的来源标签，报告另行记录实际工具源码哈希，不能用标签代替源码凭据。

另以 `--previous-release 0.2.0-alpha.1 --old-appimage /path/to/DezirClip_0.2.0-alpha.1_amd64.AppImage` 独立运行整套场景，覆盖已发布 alpha.1 到 alpha.2 的迁移。alpha.1 固定官方包 SHA256 为 `660d8d73d9b719b5c7b92f471024a715eb61fa471b56cba7364edfa1c2464628`，还须由包内实际 CLI 确认发布提交 `90ba9960744b6cd57caaaf597f68533a7e41be45`、schema 21、三个链接组件，并实际创建 schema 21 的合成历史；不接受仅将候选改名得到的“旧版本”。

## 五个场景

1. `old`：从旧 AppImage 提取真实 `dzc`，在临时 profile 写入中性的合成历史记录。
2. `read-only`：整个 scenario 只读挂载。签名验证成功，官方安装必须拒绝，旧包字节必须保持不变。
3. `replace`：修改 payload 字节、修改 manifest 版本均必须被拒绝且不得改变旧包；正确签名的候选随后由官方安装器替换旧包，并保持可执行权限。
4. `new`：从替换后的包启动实际 GUI。CLI 与 GUI 回读必须保留旧历史，WebView 必须实际渲染该条目，`get_update_support` 必须返回 `app_image` / `canInstall=true`。
5. `new-read-only`：仅当前 AppImage 只读挂载，profile 仍可写。GUI 必须继续显示历史，安装能力必须返回 `app_image_read_only` / `canInstall=false`。

`old` 写入历史后，仅在合成 profile 显式设置 `app.silent_start=false` 和 `app.disable_webview_gpu=true`，让实际 release GUI 在无 GPU 的 Xvfb 中可见。记录设置的旧值与新值，升级后的两个 GUI 场景必须保留这些值；产品默认值和真实用户配置均不改变。这项验收覆盖现有无 GPU 模式，不代表硬件加速路径已在真实桌面验证。

升级后的两个 GUI 场景另通过包内实际 `dzc build-info --json`、`dzc-service --build-info` 和 WebView 的 `get_build_info` 核对同一构建集：版本、规范 build_id、已确认的干净源码、目标、协议 1.0、schema 24 必须一致；桌面与 CLI 的四个链接组件为 Core、Platform、Protocol、Host，Service 另含自身。此项只读取 Service 元数据，不启动它。传入 `--source-revision` 时同时绑定冻结提交。完整三端元数据保留为 JSON。来源未知、修改中、版本集或字段矛盾都拒绝验收；这些静态构建信息不代替后续 IPC 身份认证。

WebView 另行保留实际 URL、DOM、页面与原生窗口可见性、Tauri bridge 和布局状态。就绪失败时尝试截图并记录截图错误，原始失败不会被重标为通过。

每个场景都有独立的 user、PID、network、IPC、UTS namespace；user/PID/network 标识还会与宿主核对。bubblewrap 清空继承的环境，使用私有 HOME、XDG profile、Xvfb 和 D-Bus，不挂载真实 HOME 或宿主桌面 socket。只有合成 scenario 可写；helper、driver、脚本和系统运行库只读挂载。只读场景进一步收紧 scenario 或 AppImage 的挂载权限。helper 拒绝非临时 fixture、错误 marker、符号链接文件和缺少隔离凭据的调用。

## 证据和边界

工具打印 `/tmp/dezirclip-native-acceptance-*/acceptance.json`。报告包括输入包、签名、公钥、helper、driver 和实际工具源码的 SHA256、逐场景命令/退出状态、时间、日志与截图哈希。成功、失败和超时均保留目录；工具不会递归删除已有目录。WebDriver 会关闭，超时仍未退出时强制终止；沙箱结束后其私有 PID namespace 中的进程随之退出。证据审阅后可由操作者按打印的精确目录自行清理。

这是本地、离线的实际包替换与 WebView 验收。它不验证生产更新服务器、生产签名密钥、线上下载，也不代表真实物理桌面的所有显示服务器行为。输入包哈希固定验收对象；候选的构建源码凭据仍需随其构建记录提供，不能从版本字符串或来源标签推断。新候选只需测试公钥和签名，不需要接触生产 secrets。实际版本变化后需显式更新 fixture 约束并重新构建、重跑；不要通过替换文件名绕过约束。
