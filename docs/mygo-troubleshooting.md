# MyGo 退出诊断

MyGo 与 Tauri 继续共用 Rust 核心。Windows MyGo 的主快捷键和托盘切换只隐藏主窗口；达到启用的空闲销毁期限后才释放 WebView。窗口关闭按钮仍按原行为关闭 WebView。

贴边悬停唤出会显示完整窗口并保留贴边位置，短暂防误隐藏时间结束后可以再次收纳；普通托盘/快捷键唤出仍避开边缘。`Super+V` 是协议中的规范名称，与 Windows 的 `Win+V` 相同。启用系统接管须先经确认让 Explorer 释放，再注册快捷键。

Service 偶发退出仍需现场证据确认。数据目录（`clipboard.db` 所在目录）包含以下诊断文件：

- `service-lifecycle.log`：启动时间、PID、正常退出或传输错误、系统错误码、Rust panic 的源码位置。超过 256 KiB 后在下一次启动轮转为 `service-lifecycle.previous.log`。
- Windows MyGo 的 `desktop-service-exit.log`：常驻进程观察到的 Host 内核退出码，可覆盖未执行 Rust panic hook 的原生崩溃或外部终止。超过 64 KiB 后在下一次记录轮转为 `.previous.log`。

这些文件不包含剪贴板正文、请求参数或 panic payload。普通隐藏不会轮询协议或写入日志；进程观察使用 Windows 内核句柄，不会自动结束任何进程。日志写入失败不阻止应用启动。

若再次出现托盘无响应，请保留这两份当前日志（必要时包括 previous 文件）和发生时间。只有 `start` 而没有 Rust 退出记录不等于确定崩溃；需结合常驻进程记录的退出码和 Windows 应用错误事件判断。常驻进程先退出时无法观察随后发生的 Service 退出。
