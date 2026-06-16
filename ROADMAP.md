# ROADMAP — tiez-clipboard

> **目的**：把调研结论固化为可执行的路线图。每项功能都给出 用户场景 / 技术方案 / 关键文件 / 依赖 / i18n / 手动 QA / 风险，使后续开发可以直接照着走。
>
> **生成时间**：2026-06-16
> **基于分支**：`slim`
> **关联文档**：[`AGENTS.md`](./AGENTS.md)（项目知识库，已提交）

---

## 0. 项目原则（约束所有未来功能）

| 原则 | 含义 |
|---|---|
| **本地优先** | 无云同步、无 MQTT、无 AI 助手、无 WebSocket、无 HTTP 客户端 |
| **平台** | 仅 Windows + Linux。`cfg(target_os = "macos")` 禁止 |
| **代码质量** | 无 `TODO/FIXME/HACK`、无 `as any`/`@ts-ignore`、无内联主题 CSS、无 cargo `[dev-dependencies]`、无前端测试框架 |
| **主题一致性** | 所有新 UI 必须适配 6 主题（`retro` / `mica` / `acrylic` / `macos` / `scifi` / `liquid-glass`）× 3 颜色模式 |
| **i18n 完整** | 新增字符串 → 先加 `zh.ts`，再 `en.ts` + `tw.ts`，跑 `npm run i18n:check` |
| **提交规范** | Conventional Commits 中文：`feat(scope): ...`、`fix(scope): ...`、`docs: ...`。slim 工作流用 `(slim)` scope |
| **构建前必跑** | Rust：`cd src-tauri && cargo test`；前端：`npm run build`（含 `tsc --strict`） |

---

## 1. 不会做的（Anti-goals）

> 提前声明避免后续反复纠结。

- 云同步 / 跨设备 / 移动端伴侣
- AI 助手 / LLM 集成
- 浏览器扩展（剪贴板桥接）
- macOS 任何代码
- 自动更新（`tauri-plugin-updater` 联网）
- 多用户 / 多 profile（单用户 + 导出/导入已足够）
- 全 Wayland 支持（受限于上游 Tauri XDG GlobalShortcuts portal PR；X11 已可用）
- 虚拟桌面固定（Win32 COM 复杂度高，受众小）
- 内嵌 Lua / 完整脚本引擎（Phase 3 之前不考虑，Rhai 风险收益不划算）

---

## 2. 功能总览

| # | 功能 | 阶段 | 工作量 | 状态 |
|---|------|------|--------|------|
| 2.1 | 二维码生成 | Phase 1 | 1 天 | 📋 planned |
| 2.2 | FTS5 + 模糊 + 正则搜索 | Phase 1 | 2-3 天 | 📋 planned |
| 2.3 | Quick-Paste 浮层 | Phase 1 | 1-2 天 | 📋 planned |
| 2.4 | 智能内容分类 + 过滤芯片 | Phase 1 | 1 周 | 📋 planned |
| 2.5 | 文本转换动作 + 右键菜单 | Phase 1 | 1 周 | 📋 planned |
| 2.6 | 屏幕区域截图到剪贴板 | Phase 2 | 3-4 天 | 📋 planned |
| 2.7 | 本地 OCR（先 Windows） | Phase 2 | 1 周/平台 | 📋 planned |
| 2.8 | 导出/导入备份 | Phase 2 | 3-5 天 | 📋 planned |
| 2.9 | tiez-c CLI 工具 | Phase 2 | 1-2 周 | 📋 planned |
| 2.9.1 | tiez-c Agent Skill（教 LLM 使用 CLI） | Phase 2 | 1-2 天（与 2.9 同步） | 📋 planned |
| — | （旧）死代码清理 | Phase 0 | 1 天 | 📋 planned |

**累计估算**：~2-3 个月专注工作量。各项可独立发布，建议按编号顺序执行（前 5 项建立"新功能印象"，后 4 项加深度）。

---

## 3. Phase 0 — 死代码清理（建议作为热身，1 天）

清理上游 fork 遗留的无效 UI/键，避免干扰后续功能。

| 位置 | 处理 |
|---|---|
| `src/locales/zh.ts` `rich_text_snapshot_preview` 键 | 删除（设置后端未实现） |
| `src/locales/zh.ts` `clipboard_item_font_size` 键 | 删除（设置未实现） |
| `src/locales/zh.ts` `sound_volume` 键 | 删除（仅 `sound_enabled` / `sound_paste_enabled` 存在） |
| `src/locales/zh.ts` `check_update` / `downloading` / `new_version_found` 键 | 删除（后端无对应命令） |
| `src/features/settings/components/SettingsFooter.tsx` "检查更新" 按钮 | 删除或改为打开浏览器到 GitHub releases 链接 |
| 任何 `app.rich_text_snapshot_preview` 引用 | 移除 |

**手动 QA**：
- [ ] 三个语言文件键数一致，`npm run i18n:check` 通过
- [ ] 设置 UI 渲染正常，删除项不留下断链

**提交**：`chore(i18n): 移除未实现的设置项 i18n 键`、`refactor(settings): 移除无效的富文本快照与更新检查入口`

---

## 4. Phase 1 — Core UX（建立"新功能印象"，1-2 月）

### 4.1 二维码生成 ⭐ 开门红

**目标平台**：Win + Linux
**用户场景**：复制了一段 URL / 文本 / Wi-Fi 配置 → 右键"生成二维码" → 弹窗预览 → 复制为图片到剪贴板 → 手机扫码获取

**技术方案**：
- **后端**：纯函数 `generate_qr_png(text: String, size_px: u32) -> Vec<u8>`
  - 依赖：`qrcode2 = "0.18"`（纯 Rust，零系统依赖）
  - 渲染路径：`QrCode::new(text)?.render::<Luma<u8>>().build()` → `image::ImageBuffer` → PNG bytes（`image` crate 已依赖）
- **前端**：
  - 复用现有 `copy_image_bytes_to_clipboard` 写回剪贴板
  - 新组件 `QrCodeDialog.tsx`：单色 280×280 预览 + "复制图片" / "保存为文件" / "关闭"
- **触发**：ClipboardItem 右键菜单添加"生成二维码"项（依赖 4.5 的右键菜单基础设施）

**关键文件**：
- 新增：`src-tauri/src/app/commands/qrcode_cmd.rs`
- 新增：`src/features/clipboard/components/QrCodeDialog.tsx`
- 新增：`src/features/clipboard/hooks/useQrCode.ts`
- 修改：`src-tauri/src/app/commands/mod.rs`（注册命令）
- 修改：`src-tauri/src/main.rs`（invoke_handler）
- 修改：`src-tauri/capabilities/default.json`（如需新权限；不需新文件 I/O，文件保存用现有 `dialog` 插件）
- 修改：`vite.config.ts`（无需新 chunk，二维码逻辑轻量）

**依赖**：新增 `qrcode2 = "0.18"`

**i18n 新增键**（zh → en/tw）：
- `clipboard.action.generate_qr` = "生成二维码"
- `clipboard.qr.title` = "二维码"
- `clipboard.qr.copy_image` = "复制图片"
- `clipboard.qr.save_file` = "保存为文件"
- `clipboard.qr.empty_input` = "无法为空内容生成二维码"

**手动 QA 场景**：
- [ ] 复制中文 / emoji / 长 URL → 二维码正确生成
- [ ] "复制图片" → 粘到画图 / GIMP → 二维码可被手机相机识别
- [ ] 6 主题下弹窗背景、按钮颜色一致（无破图、无白边、无未样式化文字）
- [ ] 弹窗 ESC 关闭，点击外部关闭（不破坏焦点）
- [ ] 极长文本（>1000 字符）不卡顿、不爆内存

**风险**：
- 二维码容量：极长文本可能超出版本限制。容错：超长时降级为 8% 容错 + 低版本（v10），仍超则提示用户
- 弹窗层级：与现有 emoji 弹窗、设置面板不冲突

**提交**：`feat(clipboard): 二维码生成（文本 → 二维码 → 剪贴板图片）`

---

### 4.2 FTS5 + 模糊 + 正则搜索

**目标平台**：Win + Linux
**用户场景**：剪贴板历史 500+ 条 → 在搜索框输入 `git` → 实时高亮匹配字符；输入 `~git` 启用模糊匹配；输入 `/^https?:\/\//` 启用正则；空时显示全部

**技术方案**：
- **数据层**：
  - `rusqlite` 已自带 FTS5（`features = ["fts5"]` 需确认，默认未启用）
  - 新建虚表 `clipboard_fts(content, preview, html_content, source_app, tags)`，`contentrowid` 关联主表
  - 同步触发器：INSERT / UPDATE / DELETE 同步
  - 启动迁移：旧数据 backfill
- **后端**：
  - 新增 `search_cmd.rs`，命令 `search_clipboard(query: SearchQuery) -> Vec<Entry>`
  - `SearchQuery { mode: Mode, term: String, limit: u32, offset: u32 }`
  - `Mode`：`Contains | Fuzzy | Regex | Fts5`
  - 模糊匹配：`fuzzy-matcher` crate 的 `SkimMatcherV2`
  - 正则：`regex` crate（已依赖）
- **前端**：
  - 搜索框 `AppHeader.tsx` 加模式指示器（图标）
  - 模式切换：默认 `Contains`；按 `Ctrl+F` 循环 `Fuzzy → Regex → Fts5 → Contains`
  - 匹配字符高亮：在 `ClipboardItem.tsx` 的 preview 文本里高亮命中子串（用 `<mark>` 元素 + 主题变量 `--tiez-accent`）
  - 搜索历史：localStorage 存最近 20 条

**关键文件**：
- 修改：`src-tauri/src/services/repository/migrations.rs`（或等价位置，加 v2 迁移）
- 新增：`src-tauri/src/app/commands/search_cmd.rs`
- 新增：`src-tauri/src/services/search.rs`（FTS5 查询构造器 + 模式分发）
- 新增：`src/features/clipboard/hooks/useSearch.ts`
- 新增：`src/features/clipboard/components/SearchBar.tsx`（拆出 AppHeader 内的搜索框）
- 修改：`src/features/clipboard/components/ClipboardItem.tsx`（匹配高亮）
- 修改：`src/locales/{zh,en,tw}.ts`

**依赖**：
- `Cargo.toml`：`rusqlite` 加 `features = ["fts5"]`（如未启用）；新增 `fuzzy-matcher = "0.3"`
- `package.json`：无新依赖

**i18n 新增键**：
- `search.mode.contains` = "包含"
- `search.mode.fuzzy` = "模糊"
- `search.mode.regex` = "正则"
- `search.mode.fts5` = "全文"
- `search.placeholder` = "搜索（输入 /regex/ 或 ~fuzzy）"
- `search.history` = "搜索历史"
- `search.clear_history` = "清空搜索历史"
- `search.no_results` = "无匹配项"

**手动 QA 场景**：
- [ ] 500 条数据下，输入即时显示结果（<50ms）
- [ ] FTS5 能匹配 "git" 找到 "git commit -m"、也能找到 "GitHub"
- [ ] 模糊 `~gcmt` 命中 "git commit"
- [ ] 正则 `/^https?:\/\/[^ ]+$/i` 命中 URL 行
- [ ] 非法正则 `/[/` 不崩溃，显示"正则语法错误"
- [ ] 搜索结果在 6 主题下高亮 `<mark>` 颜色清晰
- [ ] 删除某条历史后，搜索结果实时移除（触发器工作）
- [ ] 模糊匹配不命中时（例 `~zzzzz`）正确显示空结果
- [ ] 老数据 backfill 后可被搜到（启动一次回归）

**风险**：
- FTS5 启用增加二进制 ~200KB
- 模糊匹配性能：每条都跑 SkimMatcher 在 500 条下 < 30ms；5000 条可能需要 LRU 缓存
- 触发器与主表写入必须原子（FTS5 与业务表在同一事务）

**提交**：
- `feat(search): FTS5 全文索引 + 模糊/正则搜索 + 匹配高亮`
- `chore(db): 启动时 backfill clipboard_fts`

---

### 4.3 Quick-Paste 浮层

**目标平台**：Win + Linux（X11；Wayland 退化为新窗口）
**用户场景**：按 `Ctrl+Shift+V` → 屏幕中心浮出小窗，列 8 条最近条目 → `↑↓` 选择 → Enter 粘贴到当前焦点窗口 → Esc 关闭

**技术方案**：
- **复用现有模式**：与 `?window=compact-preview` 一样的多窗口 URL 路由
- **新窗口**：`?window=quick-paste`
  - `tauri.conf.json` 新增窗口配置：`label: "quick-paste"`, `width: 280`, `height: 360`, `alwaysOnTop: true`, `skipTaskbar: true`, `decorations: false`, `transparent: true`, `focus: false`（初始不抢焦点）
  - `main.tsx` 加路由分支
  - `capabilities/default.json` 把 `quick-paste` 加进 windows 列表
- **UI**：
  - 紧凑列表：8 条最近 + 1 条搜索输入框
  - 键盘：`↑↓` 移动、`PgUp/PgDn` 翻页、`Enter` 粘贴、`Esc` 关闭、`Tab` 切换到搜索框
  - 失焦自动关闭（`WindowEvent::Focused(false)` 触发隐藏）
- **后端**：
  - 新增 `quick_paste_cmd.rs`
  - `show_quick_paste_overlay() -> Result<()>`：调 `WebviewWindowBuilder` 创建（已存在则显示并聚焦）
  - `paste_from_overlay(entry_id) -> Result<()>`：复用现有 `paste_queue` 的"粘贴指定条目"流程
  - `hide_quick_paste_overlay() -> Result<()>`：调 `window.hide()`

**关键文件**：
- 修改：`src-tauri/tauri.conf.json`（新增 `quick-paste` 窗口）
- 修改：`src-tauri/capabilities/default.json`
- 修改：`src/main.tsx`（加 URL 路由分支）
- 新增：`src/features/quick-paste/QuickPasteWindow.tsx`
- 新增：`src/features/quick-paste/hooks/useQuickPasteKeymap.ts`
- 新增：`src/features/quick-paste/styles/quick-paste.css`（在 `src/styles/components/` 下，保持无内联 CSS）
- 新增：`src-tauri/src/app/commands/quick_paste_cmd.rs`
- 修改：`src-tauri/src/app/window_manager.rs`（复用 `show_window_no_activate`）
- 修改：`src-tauri/src/app/setup.rs`（注册全局快捷键 `Ctrl+Shift+V`）

**依赖**：无新依赖

**i18n 新增键**：
- `quick_paste.title` = "快速粘贴"
- `quick_paste.search_placeholder` = "过滤..."
- `quick_paste.hint` = "↑↓ 选择 · Enter 粘贴 · Esc 关闭"
- `quick_paste.empty` = "剪贴板无内容"

**手动 QA 场景**：
- [ ] 按 `Ctrl+Shift+V` 浮窗出现，位置不挡当前输入框
- [ ] `↑↓` 选中条目立即高亮
- [ ] Enter 粘贴到当前焦点应用（VS Code / 浏览器 / 终端各测一次）
- [ ] 浮窗打开时点击其他窗口 → 浮窗自动关闭
- [ ] 浮窗打开时再次按 `Ctrl+Shift+V` → 关闭
- [ ] 6 主题下浮窗外观一致，玻璃/实体主题都能看到边界
- [ ] 透明度设置（`app.surface_opacity`）影响浮窗
- [ ] Linux X11 下 `xdotool` 兼容性正常

**风险**：
- 焦点窃取：复用现有 `show_window_no_activate` 模式（已在 `windows_ext.rs`）
- 浮窗抢 IME 焦点：保持 `focus: false` 初始化，按键通过 `KeyDown` 事件独立处理
- Wayland：透明窗口支持受限；如不工作，UI 提示"X11 only"不报错

**提交**：`feat(clipboard): Quick-Paste 浮层（Ctrl+Shift+V 弹出最近条目）`

---

### 4.4 智能内容分类 + 过滤芯片

**目标平台**：Win + Linux
**用户场景**：剪贴板 800 条混杂 → 顶部出现芯片栏：`[全部 800] [文本 620] [代码 180] [URL 95] [JSON 42] [颜色 8] [邮箱 15] [文件路径 33]` → 点 `[代码]` 只显示代码 → 多选叠加 → 搜索框叠加更精确

**技术方案**：
- **分类器**：纯 Rust，纯启发式，无 ML
  - 在 `services/classification.rs` 新建模块
  - 输入：`&str`，输出：`Vec<ContentKind>`（一个条目可属多类）
  - 规则（按顺序短路求值）：
    1. 长度 == 0 → `Empty`
    2. 解析 JSON 成功 → `Json`
    3. 匹配 `^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$` → `Color`
    4. 匹配 UUID v1-v5 正则 → `Uuid`
    5. 匹配 RFC 5322 邮箱 → `Email`
    6. 匹配 `https?://` 或 `ftp://` → `Url`
    7. 匹配 Windows 路径 `^[A-Z]:\\` 或 Unix 路径 `^(/|\./|~/|../)` → `FilePath`
    8. 匹配 IP v4/v6 → `IpAddress`
    9. 行数 > 5 且 `{` `;` `(` 密度 > 阈值 → `Code`（粗略）
    10. 匹配手机号（中国/国际）→ `Phone`
    11. 否则 → `Text`
- **数据库**：
  - `entries` 表加 `kinds TEXT NOT NULL DEFAULT ''`（CSV 存储多类）
  - 写库前调分类器，结果存入
  - 索引：`CREATE INDEX idx_entries_kinds ON entries(kinds)`（用 LIKE 查询）
- **后端**：
  - 新增 `classification_cmd.rs`
  - `classify_content(text: String) -> Vec<ContentKind>`（实时分类，UI 预览用）
  - `reclassify_all() -> Result<usize>`（设置项变更后批量重算）
  - 现有 `clipboard_ops.rs` 写库前自动调分类
- **前端**：
  - 新组件 `FilterChips.tsx`：横排胶囊按钮，多选状态，每个 chip 显示 `图标 + 标签 + 数量`
  - 接入位置：`AppHeader.tsx` 搜索框下方
  - 6 主题统一风格：背景用 `var(--tiez-surface-2)`，选中态用 `var(--tiez-accent)` + 微透明
  - 单击切换；Cmd/Ctrl+单击多选；右键 chip 隐藏（设置项可恢复）
  - 数量 lazy 加载（窗口内仅显示当前查询命中的统计）

**关键文件**：
- 新增：`src-tauri/src/services/classification.rs`
- 新增：`src-tauri/src/app/commands/classification_cmd.rs`
- 新增：`src/features/clipboard/components/FilterChips.tsx`
- 新增：`src/features/clipboard/hooks/useFilterChips.ts`
- 新增：`src/styles/components/filter-chips.css`
- 修改：`src-tauri/src/services/repository/migrations.rs`（v3：加 kinds 列）
- 修改：`src-tauri/src/services/clipboard_ops.rs`（写库时调分类器）
- 修改：`src/features/clipboard/components/AppHeader.tsx`（插入 chip 栏）
- 修改：`src/locales/{zh,en,tw}.ts`

**依赖**：
- `Cargo.toml`：新增 `once_cell = "1"`（正则编译期缓存；或用 lazy_static / std::sync::OnceLock）
- `package.json`：无新依赖

**i18n 新增键**：
- `filter.chips.all` = "全部"
- `filter.chips.text` = "文本"
- `filter.chips.code` = "代码"
- `filter.chips.url` = "URL"
- `filter.chips.json` = "JSON"
- `filter.chips.color` = "颜色"
- `filter.chips.uuid` = "UUID"
- `filter.chips.email` = "邮箱"
- `filter.chips.filepath` = "文件路径"
- `filter.chips.ip` = "IP"
- `filter.chips.phone` = "手机号"
- `filter.chips.empty` = "空"
- `filter.hide_chip` = "隐藏此过滤"
- `filter.reset_chips` = "重置过滤"

**手动 QA 场景**：
- [ ] 复制各类内容，列表项显示对应 chip 角标
- [ ] 顶部过滤芯片数量正确
- [ ] 单击 chip → 列表过滤；Cmd+单击多选 → 列表求交集
- [ ] 右键 chip → 隐藏 → 设置面板"已隐藏过滤"区域出现该 chip → 可恢复
- [ ] 6 主题 × 3 颜色模式 = 18 种组合下 chip 视觉一致
- [ ] 模糊主题（liquid-glass、acrylic）下 chip 背景半透明可见
- [ ] 1000 条数据下 chip 计数显示 < 50ms
- [ ] 重置按钮一键清除所有 chip 选中
- [ ] 设置项 "启用智能分类" 关闭时，chip 栏消失（向后兼容）

**风险**：
- 误判：JSON 检测用 `serde_json::from_str` 严格解析，不会误判；但 Code 启发式可能误判散文
- 数据库迁移：旧数据需要回填（启动时一次性 `UPDATE entries SET kinds = classify(content) WHERE kinds = ''`）
- 性能：分类纯字符串处理，5000 条 backfill 应 < 2s

**提交**：
- `feat(clipboard): 智能内容分类（启发式 + 11 类）`
- `feat(ui): 过滤芯片栏（多选过滤）`
- `chore(db): backfill kinds 列`

---

### 4.5 文本转换动作 + 右键菜单（基础设施 + 应用）

**目标平台**：Win + Linux
**用户场景**：右键剪贴板条目 → 看到上下文菜单 → 选"文本转换" → 子菜单 `[大写] [小写] [Title Case] [camelCase] [snake_case] [去空行] [HTML → MD] [JSON 格式化] [Base64 编码] [Base64 解码] [URL 编码] [URL 解码] [SHA-256] [BLAKE3] [JWT 解码]` → 转换后生成新条目入栈

**技术方案**：

**右键菜单基础设施**（一次性投入，多功能复用）：
- 新组件 `ItemContextMenu.tsx`：基于 Radix UI `ContextMenu` 或自研（项目目前未用 Radix，需评估）
  - 决策：项目当前右键菜单是 HTML + JS 事件；评估后发现 Radix ContextMenu 与现有 CSS 主题系统集成较复杂，**优先自研**（200-300 LOC），与项目 6 主题风格完全可控
  - 入口：ClipboardItem 右键触发（`onContextMenu` 阻止默认 + 自定义位置）
  - 渲染：fixed 定位 + backdrop blur（用 `var(--tiez-surface-1)`）
- 子菜单用 hover 延迟展开（300ms）+ 键盘 `→` 展开 + `←` 收回
- 菜单项元数据统一结构：`{ id, labelKey, icon, requiresContentType, action: 'transform' | 'command' | 'submenu' }`

**文本转换功能**：
- **后端** `src-tauri/src/services/transforms.rs`：
  - `apply_transform(text: String, transform_id: TransformId) -> Result<TransformedOutput>`
  - 一次性注册所有 transform，每个是纯函数
  - 转换 ID 枚举：`ToUpper | ToLower | ToTitle | ToCamel | ToSnake | TrimLines | StripBlank | HtmlToMd | MdToHtml | JsonFormat | JsonMinify | Base64Encode | Base64Decode | UrlEncode | UrlDecode | Sha256 | Blake3 | JwtDecode | DiffWithPrevious`
  - 转换结果：要么是新文本（生成新条目），要么是元数据（diff 用）
- **依赖**：
  - `pulldown-cmark = "0.10"` — Markdown ↔ HTML
  - `html2md` 或自实现 HTML → MD（用 `html5ever` + `markup5ever_rcdom`）— 选 `html2md = "0.2"` 简单够用
  - `similar = "3"` — diff
  - `blake3 = "1"` — 哈希
  - `jsonwebtoken = "9"` — JWT 解码（仅 decode，不验签）
  - 其他：现有 `regex`、`base64`、`urlencoding`、`serde_json`、`sha2`
- **前端**：
  - `ItemContextMenu.tsx` 调用 `invoke('apply_transform', { text, transformId })`
  - 转换结果：新文本 → 调 `add_clipboard_item`（写入历史）；diff → 弹窗显示

**关键文件**：
- 新增：`src-tauri/src/services/transforms.rs`（转换函数注册表）
- 新增：`src-tauri/src/app/commands/transform_cmd.rs`
- 新增：`src/features/clipboard/components/ItemContextMenu.tsx`
- 新增：`src/features/clipboard/components/transforms/TransformMenuItems.tsx`（菜单项定义）
- 新增：`src/features/clipboard/hooks/useItemActions.ts`（菜单行为 hook）
- 新增：`src/styles/components/context-menu.css`
- 修改：`src/features/clipboard/components/ClipboardItem.tsx`（挂载右键事件）
- 修改：`src-tauri/src/app/commands/mod.rs`（注册命令）
- 修改：`src-tauri/src/main.rs`（invoke_handler）
- 修改：`src/locales/{zh,en,tw}.ts`

**依赖**：
- `Cargo.toml` 新增：`pulldown-cmark = "0.10"`、`html2md = "0.2"`、`similar = "3"`、`blake3 = "1"`、`jsonwebtoken = "9"`
- `package.json` 无新依赖（不引 Radix）

**i18n 新增键**（每项转换 + 上下文菜单）：

转换标签：
- `transform.menu` = "文本转换"
- `transform.to_upper` = "转大写"
- `transform.to_lower` = "转小写"
- `transform.to_title` = "Title Case"
- `transform.to_camel` = "camelCase"
- `transform.to_snake` = "snake_case"
- `transform.to_kebab` = "kebab-case"
- `transform.trim_lines` = "去除行尾空白"
- `transform.strip_blank` = "删除空行"
- `transform.dedupe_lines` = "去重行"
- `transform.sort_lines` = "排序行"
- `transform.html_to_md` = "HTML → Markdown"
- `transform.md_to_html` = "Markdown → HTML"
- `transform.json_format` = "JSON 格式化"
- `transform.json_minify` = "JSON 压缩"
- `transform.base64_encode` = "Base64 编码"
- `transform.base64_decode` = "Base64 解码"
- `transform.url_encode` = "URL 编码"
- `transform.url_decode` = "URL 解码"
- `transform.sha256` = "SHA-256 哈希"
- `transform.blake3` = "BLAKE3 哈希"
- `transform.jwt_decode` = "JWT 解码"
- `transform.diff_prev` = "与上一条同类型对比"

菜单项：
- `context_menu.copy` = "复制"
- `context_menu.paste` = "粘贴到此处"
- `context_menu.pin` = "置顶"
- `context_menu.unpin` = "取消置顶"
- `context_menu.delete` = "删除"
- `context_menu.edit` = "编辑"
- `context_menu.generate_qr` = "生成二维码"
- `context_menu.transform` = "文本转换"
- `context_menu.tag` = "标签"
- `context_menu.export` = "导出为文件"

**手动 QA 场景**：
- [ ] 复制 `Hello World` → 右键 → 转大写 → 新条目 `HELLO WORLD` 出现在顶部
- [ ] 复制 JSON 字符串 → 格式化 → 弹窗显示格式化结果（4 空格缩进）
- [ ] 复制非法 JSON → 格式化 → 错误提示"无法解析为 JSON"，不崩溃
- [ ] 复制长文本 → Base64 编码 → 解码 → 还原与原文一致
- [ ] 复制 JWT token → 解码 → 弹窗显示 header / payload / 不验签
- [ ] 复制含多空行文本 → 去空行 → 紧凑
- [ ] 6 主题下右键菜单样式一致：背景半透明、选中态高亮、滚动条颜色
- [ ] 右键菜单键盘导航：`↑↓` 移动、Enter 触发、Esc 关闭、→ 进入子菜单、← 返回
- [ ] 转换后写入历史不污染原有排序（按 `move_to_top_after_paste` 设置）
- [ ] 大文本（>1MB）转换不卡 UI（用 `tokio::task::spawn_blocking` 或 Tauri async）

**风险**：
- 同步阻塞：转换在 `tauri::command` 异步线程跑，大文本 JSON 格式化可能慢；用 `spawn_blocking`
- 内存：超大文本（>10MB）转换可能 OOM；前置检查 `content.len() < 5MB`
- 转换误操作：转换后写入历史是新的条目，不修改原条目（可恢复）

**提交**：
- `feat(clipboard): 文本转换动作（17 种转换 + 应用后入栈）`
- `feat(ui): 条目右键菜单（自研 + 6 主题适配）`

---

## 5. Phase 2 — 内容处理与备份（增加深度，2-3 月）

### 5.1 屏幕区域截图到剪贴板

**目标平台**：Win + Linux（X11 完整，Wayland 退化）
**用户场景**：按 `Ctrl+Shift+A` → 屏幕变暗，鼠标变十字 → 拖拽选区 → 松开鼠标 → 截图直接进剪贴板；同时可选 `OCR 提取文字` 流程（依赖 5.2）

**技术方案**：
- **后端**：
  - 新增 `screenshot_cmd.rs`
  - `capture_screen() -> Result<DynamicImage>`：调 `xcap` 拿全屏截图
  - `capture_region(bounds: Rect) -> Result<DynamicImage>`：先全屏裁剪
  - 复用现有 `copy_image_bytes_to_clipboard` 写回剪贴板
- **覆盖层 UI**：
  - 新窗口 `?window=region-select`：全屏透明，`focus: false`, `decorations: false`, `transparent: true`, `skipTaskbar: true`, `alwaysOnTop: true`
  - 渲染：CSS 蒙层 `rgba(0,0,0,0.3)` + 鼠标位置实时矩形选区高亮（`var(--tiez-accent)` 边框）
  - 鼠标事件：`mousedown` 起、`mousemove` 更新、`mouseup` 提交 bounds
  - 提交后通过 Tauri 事件 `region-selected` 把 bounds 发到主进程
- **Wayland 处理**：
  - 失败时静默降级为"全屏截图"（仍可用）
  - 设置项 "区域截图（实验性）" 关闭时直接走全屏

**关键文件**：
- 新增：`src-tauri/src/app/commands/screenshot_cmd.rs`
- 新增：`src-tauri/src/services/screenshot.rs`（xcap 封装 + 平台选择）
- 新增：`src/features/region-select/RegionSelectWindow.tsx`
- 新增：`src/features/region-select/hooks/useRegionSelect.ts`
- 新增：`src/styles/components/region-select.css`
- 修改：`src-tauri/tauri.conf.json`（新增 `region-select` 窗口）
- 修改：`src-tauri/capabilities/default.json`
- 修改：`src/main.tsx`（加路由分支）
- 修改：`src-tauri/src/app/setup.rs`（注册 `Ctrl+Shift+A` 快捷键）
- 修改：`src/locales/{zh,en,tw}.ts`

**依赖**：新增 `xcap = "0.0"`（跨平台截图，注意版本号实际为 `xcap = "0.0.13"` 之类，需查最新稳定）

**i18n 新增键**：
- `screenshot.hint` = "拖拽鼠标选择区域 · Esc 取消"
- `screenshot.region_capture` = "区域截图"
- `screenshot.full_capture` = "全屏截图"
- `screenshot.captured_toast` = "已复制到剪贴板"
- `screenshot.unsupported_wayland` = "Wayland 下区域截图需要 xdg-desktop-portal"

**手动 QA 场景**：
- [ ] `Ctrl+Shift+A` → 全屏蒙层出现，鼠标变十字
- [ ] 拖拽 200×100 区域 → 松开 → 截图进剪贴板
- [ ] 粘到画图 / Paint / GIMP → 截图内容正确
- [ ] 多显示器：蒙层覆盖所有显示器（spanned fullscreen）
- [ ] 高 DPI：截图不模糊，物理像素与逻辑像素正确映射
- [ ] `Esc` 取消
- [ ] 截图同时触发 OCR（如 5.2 已完成）
- [ ] Linux X11：截图包含 X11 覆盖层（如有 GUI 调试工具）/ 不含自身蒙层（用 xdotool 顺序保证）
- [ ] Linux Wayland：自动降级，弹 Toast 提示

**风险**：
- 截图自包含：Linux X11 下用 `xdotool` 顺序确保蒙层在截图 API 之后显示（避免蒙层出现在截图里）
- Wayland：必须经 xdg-desktop-portal，会弹原生授权对话框（一次性）；UI 需明确告知用户
- 多显示器：不同 DPI 缩放下 bounds 转换要正确
- 隐私：用户应能关闭该功能（设置项默认开启但可关）

**提交**：
- `feat(clipboard): 屏幕区域截图（xcap + 自研选区蒙层）`
- `feat(hotkey): Ctrl+Shift+A 区域截图快捷键`

---

### 5.2 本地 OCR

**目标平台**：先 Windows（依赖 `windows` crate 已存在），后 Linux（Tesseract WASM）
**用户场景**：复制了一张含文字的截图到剪贴板 → 列表里该图片项显示 OCR 提取的文字（缩略预览）→ 搜索 "TODO" 时这张图片也能命中 → 右键"复制提取文字"

**技术方案**：
- **Windows（优先）**：
  - 使用 `Windows.Media.Ocr` UWP API
  - 依赖：`windows` crate 已依赖，加 feature flag `Media_Ocr`
  - 调用：`OcrEngine::TryCreateFromUserProfileLanguages()` → `RecognizeAsync(image)`
  - 图像源：把 `DynamicImage` 转成 `SoftwareBitmap`
- **Linux（后续）**：
  - Tesseract WASM 模式：避免 C++ 构建链
  - 把 `tesseract.js` 的 `.wasm` 和 `eng.traineddata` 打包到 `src-tauri/resources/`
  - 在 WebView 端用 Web Worker 跑 OCR（避免阻塞 UI）
  - 把结果通过 Tauri 事件发回 Rust 存储
- **数据层**：
  - `entries` 表加 `ocr_text TEXT` 和 `ocr_status TEXT`（`pending` / `done` / `failed` / `unsupported`）
  - 写库时如果是图片 → 异步触发 OCR（不阻塞剪贴板监听）
  - OCR 完成后 `UPDATE entries SET ocr_text = ?, ocr_status = 'done' WHERE id = ?`
  - FTS5 虚表同步加 `ocr_text` 列（v4 迁移）
- **后端**：
  - 新增 `ocr_cmd.rs`
  - `extract_text_from_image(entry_id) -> Result<String>`（同步入口）
  - `reprocess_all_images() -> Result<usize>`（设置项触发重跑）
  - `ocr_engine_status() -> Result<EngineInfo>`（前端显示"中文 / 英文"可用语言）

**关键文件**：
- 新增：`src-tauri/src/app/commands/ocr_cmd.rs`
- 新增：`src-tauri/src/services/ocr/`（平台分发）
- 新增：`src-tauri/src/services/ocr/windows.rs`
- 新增：`src-tauri/src/services/ocr/linux.rs`（占位，Tesseract WASM 后续）
- 修改：`src-tauri/src/services/repository/migrations.rs`（v4：加 ocr_text + ocr_status + 同步 FTS5）
- 修改：`src-tauri/src/services/clipboard_ops.rs`（图片写库后触发 OCR）
- 修改：`src/features/clipboard/components/ClipboardItem.tsx`（图片项显示 ocr_text 缩略）
- 修改：`src/locales/{zh,en,tw}.ts`
- 新增资源（Linux 阶段）：`src-tauri/resources/tesseract/{tesseract-core.wasm, eng.traineddata}`

**依赖**：
- `Cargo.toml`：`windows` crate 加 `Media_Ocr` feature（Windows-only cfg）
- `Cargo.toml`（Linux 阶段）：无需新增
- `package.json`（Linux 阶段）：Tesseract.js WASM 资源（拷贝到 public/）

**i18n 新增键**：
- `ocr.menu.extract` = "提取文字"
- `ocr.menu.copy_text` = "复制提取文字"
- `ocr.status.pending` = "OCR 处理中..."
- `ocr.status.done` = "已提取"
- `ocr.status.failed` = "识别失败"
- `ocr.status.unsupported` = "此系统不支持 OCR"
- `ocr.search_hint` = "（含图片提取文字）"
- `ocr.reprocess_all` = "重新识别所有图片"

**手动 QA 场景**：
- [ ] 复制一张含 "TODO" 文字的截图 → 1-2 秒后列表项显示缩略文字
- [ ] 在搜索框输入 "TODO" → 该图片条目出现在结果里
- [ ] 复制纯文字截图（含中文）→ OCR 提取正确率 > 80%
- [ ] 复制模糊 / 倾斜的图 → OCR 状态显示 "failed" 或部分识别
- [ ] 设置关闭"图片 OCR" → 不再处理新图片
- [ ] 重启应用 → OCR 缓存不丢失（DB 持久化）
- [ ] Linux X11（无 OCR）：状态显示 "unsupported"，UI 隐藏 OCR 相关按钮
- [ ] 隐私：OCR 数据不离开本机（明确告知用户）

**风险**：
- Windows 权限：`Windows.Media.Ocr` 需要 Win10+，且系统语言包要装
- Linux 包大小：Tesseract.js WASM + traineddata ~30MB（仅 Linux 用户承担；可做成可选资源）
- 性能：单图 1-3 秒是正常的；用 `tokio::task::spawn_blocking` 避免阻塞
- 准确率：用户应理解 OCR 不完美（UI 显示"识别结果仅供参考"）

**提交**：
- `feat(ocr): Windows.Media.Ocr 本地图片文字识别（先 Win）`
- `chore(db): v4 迁移：ocr_text + ocr_status + FTS5 同步`

---

### 5.3 导出/导入备份

**目标平台**：Win + Linux
**用户场景**：设置 → "数据管理" → "导出历史" → 选 JSON / 加密 JSON → 选位置保存 → 验证：用 tiez-c import 还原一致

**技术方案**：
- **导出格式**：
  - 基础：`tiez-export-v1.json` —— JSON `{ version, exported_at, entries: [...] }`
  - 加密：同上 + AES-GCM 加密（用 `keyring` 存的主密钥），文件后缀 `.tiezbak`
- **条目字段**（按需）：`id, content_type, content, preview, html_content, source_app, source_app_path, created_at, updated_at, use_count, is_pinned, pinned_order, tags, ocr_text, kinds`
- **加密数据**：
  - Windows：复用现有 `dpapi:` 前缀
  - Linux：复用现有 `aes-gcm` 路径
  - 跨平台：用独立 passphrase（导出时用户输入）；Argon2id 派生密钥
- **后端**：
  - `export_cmd.rs`：`export_to_file(path, format: 'json'|'encrypted', options: ExportOptions) -> Result<ExportSummary>`
  - `import_cmd.rs`：`import_from_file(path, mode: 'merge'|'replace') -> Result<ImportSummary>`
  - 合并模式：按 `id` upsert；替换模式：清空后导入
- **前端**：
  - `DataSettingsGroup.tsx` 新增"导出" / "导入" 按钮 + 进度条
  - 导入前显示预览（前 10 条）
  - 加密导出用模态框输入 passphrase

**关键文件**：
- 新增：`src-tauri/src/app/commands/export_cmd.rs`
- 新增：`src-tauri/src/app/commands/import_cmd.rs`
- 新增：`src-tauri/src/services/backup.rs`（序列化 / 反序列化 / 加密 / 校验）
- 修改：`src/features/settings/components/groups/DataSettingsGroup.tsx`
- 修改：`src/locales/{zh,en,tw}.ts`
- 修改：`src-tauri/capabilities/default.json`（如需新权限；文件对话框用现有 `dialog` 插件）

**依赖**：
- `Cargo.toml` 新增：`argon2 = "0.5"`（仅加密导出时用）
- `Cargo.toml` 新增：`zeroize = "1"`（密钥清零）
- `package.json` 无新依赖

**i18n 新增键**：
- `data.export.title` = "导出历史"
- `data.export.json` = "JSON 明文"
- `data.export.encrypted` = "加密备份（passphrase）"
- `data.export.choose_format` = "选择导出格式"
- `data.export.passphrase_prompt` = "输入加密密码（至少 12 位）"
- `data.export.confirm_passphrase` = "再次输入密码"
- `data.export.success` = "已导出 {count} 条"
- `data.import.title` = "导入历史"
- `data.import.merge` = "合并"
- `data.import.replace` = "替换"
- `data.import.preview` = "预览前 10 条"
- `data.import.success` = "已导入 {count} 条，跳过 {skipped} 条"
- `data.import.passphrase_prompt` = "输入解密密码"
- `data.import.wrong_passphrase` = "密码错误或文件已损坏"
- `data.import.version_mismatch` = "备份文件版本不兼容"

**手动 QA 场景**：
- [ ] 导出 JSON → 文件存在 → 解析为合法 JSON → 含 `version` + `entries` 字段
- [ ] 导出加密 → 输入密码 12+ 位 → 重输一致 → 文件不可被文本编辑器读取
- [ ] 导入 JSON（合并）→ 现有条目 + 导入条目共存
- [ ] 导入 JSON（替换）→ 现有条目清空，新条目入库
- [ ] 导入错误密码 → 提示"密码错误"，不破坏现有数据
- [ ] 导入版本不匹配（手工改 `version` 字段）→ 友好提示
- [ ] 导入大量数据（10k 条）→ 进度条更新，< 30s 完成
- [ ] 6 主题下导入/导出对话框样式一致
- [ ] 跨设备：Linux 导出的加密备份能在 Windows 导入（用 passphrase 解密）

**风险**：
- 序列化大小：含 `content` 字段可能很大；分批写入 + 进度回调
- 加密导出在 Linux 上没有 keyring 时的 fallback：纯 passphrase 模式（要求至少 12 位）
- 导入安全性：导入 JSON 不应执行任何代码（仅反序列化数据）

**提交**：
- `feat(data): 导出 / 导入历史（JSON + AES-GCM 加密）`
- `feat(data): 加密导出 passphrase 派生（Argon2id）`

---

### 5.4 tiez-c CLI 工具

**目标平台**：Win + Linux（独立二进制）
**用户场景**：

```bash
# SSH：从远程服务器拉取本地剪贴板最近一条
$ ssh me@server 'echo "$(tiez-c get --latest)"'

# FZF 集成：从 500 条历史里选一条复制到剪贴板
$ tiez-c list --ids | fzf --preview 'tiez-c get {}' | xargs -I {} tiez-c get {} | xclip

# 脚本：监控剪贴板，命中 TODO 时通知
$ tiez-c watch --json | jq -r 'select(.preview | test("TODO")) | .id'

# 快速添加：把命令历史转成剪贴板条目
$ echo "git commit -m 'fix:...'" | tiez-c add -

# 统计
$ tiez-c stats
total: 1283
by_type:
  text: 1100
  image: 95
  file: 88
most_used: "git checkout" (47 times)
```

**技术方案**：
- **独立 cargo binary**：`src-tauri/src/bin/tiez-c.rs`
  - `Cargo.toml` 加 `[[bin]]` 段（默认已存在 main.rs；加 name = "tiez-c"）
  - 复用现有 `services/repository` 模块（直接读 SQLite）
- **CLI 框架**：`clap = "4"`（derive 模式）
- **命令结构**：
  - `tiez-c list [OPTIONS] [N]` —— 列最近 N 条（默认 20）
    - `--type <KIND>` 过滤类型
    - `--tag <TAG>` 过滤标签
    - `--pinned` 只显示置顶
    - `--json` JSON 输出
    - `--ids` 仅 ID（一行一个）
    - `--quiet` 无图标（脚本友好）
  - `tiez-c search <QUERY>` —— 全文搜索（用 FTS5 / 模糊 / 正则；与 4.2 共用查询引擎）
  - `tiez-c get <ID|latest>` —— 打印条目内容
  - `tiez-c add <TEXT|->` —— 添加条目（`@file` 从文件，`-` 从 stdin）
  - `tiez-c delete <ID>` —— 删除
  - `tiez-c pin <ID>` / `tiez-c unpin <ID>`
  - `tiez-c tag add <ID> <TAG>` / `tiez-c tag remove <ID> <TAG>` / `tiez-c tag list`
  - `tiez-c export <FILE>` / `tiez-c import <FILE>` —— 调 5.3 的 backup 模块
  - `tiez-c stats` —— 统计
  - `tiez-c watch [OPTIONS]` —— 持续输出新条目（FIFO 模式）
- **输出格式**：
  - 默认：人类可读（`[TEXT] VSCode 2026-06-16 21:30 #142  use 5x  pinned`）
  - `--json`：`{ id, type, content, preview, source_app, created_at, use_count, is_pinned, tags }`
- **可执行性**：
  - 用户安装路径：`src-tauri/target/release/tiez-c`（与 tiez-clipboard 同 bundle 目录）
  - 文档：`docs/cli.md` + `tiez-c --help`
  - NSIS / deb / AppImage / rpm 安装时把 tiez-c 放到 `/usr/local/bin/`（或在用户 PATH 内）

**关键文件**：
- 新增：`src-tauri/src/bin/tiez-c.rs`
- 新增：`src-tauri/src/bin/cli/mod.rs`（命令树）
- 新增：`src-tauri/src/bin/cli/{list,search,get,add,delete,tag,export,import,stats,watch}.rs`
- 新增：`docs/cli.md`（用户文档）
- 修改：`src-tauri/Cargo.toml`（加 `[[bin]]` + `clap` 依赖）
- 修改：`src-tauri/src/services/repository/mod.rs`（暴露 CLI 需要的查询函数）
- 修改：`src-tauri/tauri.conf.json`（`bundle.targets` 加 tiez-c 二进制）
- 修改：`src-tauri/nsis/installer.nsi`（如有自定义安装器）
- 修改：`.github/ISSUE_TEMPLATE/*`（如需新增 "cli" 标签）

**依赖**：
- `Cargo.toml` 新增：`clap = { version = "4", features = ["derive"] }`
- `Cargo.toml` 新增：`serde_json`（已有）
- `Cargo.toml` 新增：`chrono`（已有）
- 其他全部复用现有依赖

**i18n**：CLI 文本默认中文（与项目语言一致），但用 `clap` 的本地化可能性预留（不强制要求）

**手动 QA 场景**：
- [ ] `tiez-c --help` 输出清晰命令树
- [ ] `tiez-c list` 输出最近 20 条，每条含类型图标 / 时间 / 来源
- [ ] `tiez-c list --json` 输出合法 JSON，`jq` 可解析
- [ ] `tiez-c list --ids | fzf` 可正常用键盘选择
- [ ] `tiez-c search "git"` 命中含 "git" 的条目
- [ ] `tiez-c add "hello"` 添加到历史，GUI 应用中可见
- [ ] `tiez-c add -` 从 stdin 读
- [ ] `tiez-c add @file.txt` 从文件读
- [ ] `tiez-c get <id>` 打印内容
- [ ] `tiez-c pin/unpin` 状态切换，GUI 同步
- [ ] `tiez-c watch` 持续输出新条目（GUI 复制时 CLI 立即打印）
- [ ] `tiez-c stats` 输出统计
- [ ] `tiez-c export` / `tiez-c import` 与 5.3 文件格式兼容
- [ ] 无 GUI 运行（只启动 tiez-c）→ 不依赖 Tauri runtime，直接读 DB
- [ ] Linux / Windows 各测一次
- [ ] 终端编码（中文 / emoji）显示正确

**风险**：
- DB 锁冲突：tiez-c 与 GUI 同时写 SQLite 时，WAL 模式天然支持并发读 + 单写者；写操作排队即可
- 二进制体积：增加 ~1MB（含 clap）
- 文档：tiez-c 是个独立工具，README/AGENTS.md 应有专门段落

**提交**：
- `feat(cli): tiez-c 独立 CLI 工具（list/search/get/add/pin/tag/export/import/stats/watch）`
- `docs: tiez-c 使用文档`

---

### 5.4.1 tiez-c Agent Skill（教 LLM 使用 CLI）

**目的**：在仓库内随项目发布一个 LLM 友好的技能文件（`SKILL.md`），让 Claude / GPT / 其他代理在不查阅 `docs/cli.md` 的前提下，能正确使用 `tiez-c` 完成"读 / 搜 / 增 / 标 / 导"等任务。

**目标平台**：Win + Linux（与 CLI 同步）

**用户场景**（LLM 视角）：

| 用户说 | LLM 应执行 |
|---|---|
| "我上周从 VS Code 复制过一条 URL，帮我找出来" | `tiez-c list --source VSCode --json \| jq '...'` 或 `tiez-c search "..."` |
| "把这段 markdown 存进剪贴板历史" | `echo "..." \| tiez-c add -` |
| "找出包含 'TODO' 的最近 10 条" | `tiez-c search "TODO" --limit 10` |
| "给我看现在剪贴板里最常用的 5 个文本片段" | `tiez-c stats --top 5` |
| "把第 142 条加上 'work' 标签" | `tiez-c tag add 142 work` |
| "导出最近一个月到 ~/backup.json" | `tiez-c export ~/backup.json --since "30 days ago"` |
| "我看不清 800 条历史，按代码过滤一下" | `tiez-c list --kind code`（依赖 4.4 智能分类） |
| "把这条转成大写" | `tiez-c get 142 \| tiez-c transform upper \| tiez-c add -` |

**技术方案**：

**Skill 文件结构**（标准 OpenCode / Claude 技能格式）：

```
skills/
└── tiez-c-cli/
    ├── SKILL.md           # 技能定义（YAML frontmatter + 内容）
    ├── examples/
    │   ├── search-and-add.md
    │   ├── fzf-pick.md
    │   ├── bulk-tag.md
    │   └── watch-trigger.md
    └── install.sh         # 用户级安装脚本（软链接到 ~/.claude/skills/）
```

**`SKILL.md` 内容大纲**：

1. **YAML frontmatter**：
   ```yaml
   ---
   name: tiez-c
   description: Use the tiez-c CLI to read, search, and manage the tiez-clipboard local clipboard history. Triggers: "clipboard history", "find what I copied", "search clipboard", "add to clipboard", "pin/tag/export clipboard items", "stats about clipboard usage". Pure-local operations; no network.
   ---
   ```

2. **When to use this skill**（决策清单）：
   - 用户提到 "剪贴板" / "clipboard" / "我复制过" / "粘贴板" / "clip history"
   - 用户要求查找最近复制过的内容
   - 用户要求把内容加入剪贴板
   - 用户要求管理剪贴板历史（标签 / 置顶 / 删除 / 导出）
   - **不要**在以下情况使用：用户只想要把 X 复制到当前光标位置（这是 GUI 粘贴，应让 GUI 处理）

3. **Quick reference**：
   ```text
   READ:  list / search / get / stats / tag list
   WRITE: add / pin / unpin / delete / tag add|remove / export / import / transform
   WATCH: watch
   ```

4. **Output conventions**（重要，避免 LLM 误解析）：
   - 默认输出**人类可读**，含 Unicode 类型图标。LLM 解析用 `--json` 或 `--ids` 或 `--quiet`
   - JSON 模式字段：`{ id, type, content, preview, source_app, created_at, use_count, is_pinned, tags, kinds }`
   - 时间戳统一 ISO 8601 + 本地时区

5. **Common patterns**（5-8 个最常用模式）：
   - 搜索：list 5 条候选 → LLM 选 → get
   - 模糊搜索：用 `search --mode fuzzy "term"`
   - 时间范围：`list --since "2026-06-01"`
   - 组合：`search "TODO" --source VSCode --kind code`
   - 管道：`tiez-c watch --json | jq ...`（实时触发）

6. **Safety & permissions**：
   - **零网络**：所有操作本地（强化提示，匹配 fork 哲学）
   - **无代码执行**：tiez-c 不接受 `--exec` / `--shell` / 类似参数；不接受 stdin 之外的任意代码
   - **可逆**：
     - `add` / `pin` / `tag` 可在 GUI 撤销
     - `delete` **不可逆**（除非有备份）—— LLM 在调用前应明确告知用户
     - `import --mode replace` **不可逆** —— 必须二次确认
   - **大内容防护**：超过 1MB 的内容不应 `get` 后整段塞进 LLM 上下文；用 `get <id> --preview` 或仅打印 `preview` 字段

7. **Worked examples**（每例独立文件 `examples/*.md`）：
   - `search-and-add.md`："找最近的密码格式字符串并加 sensitive 标签"
   - `fzf-pick.md`："给用户 5 选 1 的交互选择"
   - `bulk-tag.md`："把今天所有 URL 打上 'web' 标签"
   - `watch-trigger.md`："监听新条目，命中规则时通知"
   - `export-encrypted.md`："导出加密备份的完整流程"

8. **Failure modes & recovery**：
   - `tiez-c: database is locked` → GUI 在写；tiez-c 自动重试 3 次，每次 100ms；如失败，提示用户
   - `tiez-c: not found` → 检查 `$PATH`，提示运行 `install.sh` 或将 `target/release/tiez-c` 加入 PATH
   - `search: empty result` → 切到 `search --mode fuzzy` 重试
   - `add: content too large` → 超过 `MAX_PERSISTED_TEXT_BYTES`（10MB），建议分批

**分发与安装**：

- **随仓库发布**：`skills/tiez-c-cli/` 完整目录在 git 内
- **用户级安装**（推荐）：
  ```bash
  # Linux
  ./skills/tiez-c-cli/install.sh
  # → ln -s "$(pwd)/skills/tiez-c-cli" ~/.claude/skills/tiez-c-cli
  # Windows (PowerShell)
  .\skills\tiez-c-cli\install.ps1
  # → New-Item -ItemType Junction -Path "$env:USERPROFILE\.claude\skills\tiez-c-cli" -Target "$(Resolve-Path .\skills\tiez-c-cli)"
  ```
- **其他代理支持**：在 `SKILL.md` 头部加注释，说明本技能同时适用于 Claude / Cursor / Continue / OpenCode 等支持 OpenCode Skills 规范的代理
- **版本同步**：在 `SKILL.md` frontmatter 加 `tiez-clipboard-version: ^0.x` 约束；不匹配时技能加载器应提示

**关键文件**：
- 新增：`skills/tiez-c-cli/SKILL.md`
- 新增：`skills/tiez-c-cli/examples/{search-and-add,fzf-pick,bulk-tag,watch-trigger,export-encrypted}.md`
- 新增：`skills/tiez-c-cli/install.sh`（Linux/macOS）
- 新增：`skills/tiez-c-cli/install.ps1`（Windows）
- 新增：`skills/tiez-c-cli/README.md`（开发者向：如何更新本技能）
- 修改：`README.md`（"开发者"段落链接到 `skills/tiez-c-cli/`）
- 修改：`docs/cli.md`（末尾加 "作为 LLM 技能使用" 小节）

**依赖**：无新依赖（SKILL.md 是纯文档）

**i18n**：SKILL.md 默认英文（与国际 LLM 代理生态一致）；中文版在 `SKILL.zh.md` 备选

**手动 QA 场景**：

1. **技能发现**：
   - [ ] 用户问 "我上周复制过的 docker 命令在哪" → Claude 自动加载 `tiez-c` skill
   - [ ] 用户问 "把这条加到剪贴板历史" → Claude 调 `tiez-c add`

2. **实际执行**（手工 LLM 测试）：
   - [ ] 给 Claude 5 个真实查询，验证返回的命令正确
   - [ ] 给 Claude 1 个"危险"查询（"删掉所有 30 天前的"），验证先问确认

3. **技能自包含**：
   - [ ] 删除用户文档 `docs/cli.md`，仅靠 `SKILL.md` LLM 仍能完成 80% 任务
   - [ ] SKILL.md 不引用未公开的内部 API / 未实现的命令

4. **install 脚本**：
   - [ ] Linux：`./install.sh` 后 `~/.claude/skills/tiez-c-cli/SKILL.md` 存在且为软链接
   - [ ] Windows：`install.ps1` 后 junction 存在，可正常读取
   - [ ] 卸载：`./install.sh --uninstall` 清理软链接

5. **跨代理兼容**：
   - [ ] Claude Code（已带）→ 加载成功
   - [ ] Cursor（自定义规则路径不同）→ 在 SKILL.md 注释中说明
   - [ ] OpenCode（`~/.config/opencode/skills/`）→ 提供替代安装路径

6. **版本同步**：
   - [ ] tiez-c 加新命令后，SKILL.md 的 quick reference 同步更新
   - [ ] README / docs/cli.md / SKILL.md 三者描述的命令集一致

**风险**：
- 技能格式差异：不同 LLM 代理的 SKILL.md schema 略有不同；本项目以 OpenCode Skills 规范为基线（与 AGENTS.md 风格一致）
- LLM 误用：`tiez-c delete --all` 之类灾难性命令必须要求 `--yes` 二次确认；`import --mode replace` 必须显示条目数等用户确认
- 安全性 vs 易用性：默认 **不** 接受任意 stdin 之外的代码；LLM 应通过组合命令实现"transform + add"，而不是 shell 注入

**提交**：
- `docs(skills): tiez-c Agent Skill（SKILL.md + 5 examples + install.sh/ps1）`
- `docs: README 链接到 skills/tiez-c-cli/`

---

## 6. 跨功能基础设施

### 6.1 右键菜单系统（4.5 完成，4.1 + 5.1 + 5.2 复用）

- 单一组件 `ItemContextMenu.tsx`，所有功能通过菜单项 manifest 注册
- 菜单项分组：`[基础操作] [文本转换] [生成] [标签] [危险操作]`
- 6 主题适配通过 `var(--tiez-surface-*)` + `var(--tiez-accent)` 完成

### 6.2 异步任务进度反馈

OCR / 截图 / 导入导出等长任务用统一模式：
- 前端：`<ProgressToast />` 浮窗组件
- 后端：`tauri::async_runtime::spawn` + `app.emit("task-progress", { id, percent, message })`
- 已有的 `tiez.emit(...)` 事件机制直接复用

### 6.3 i18n key 命名规范

为保持一致性：
- 功能前缀：`clipboard.*` / `search.*` / `quick_paste.*` / `filter.*` / `transform.*` / `screenshot.*` / `ocr.*` / `data.*` / `cli.*` / `settings.*`
- 通用：`common.cancel` / `common.confirm` / `common.error` / `common.success`
- 新增键时先建 `zh.ts`，再 `en.ts` + `tw.ts`，最后 `npm run i18n:check`

---

## 7. 评估标准（"完成" 意味着什么）

每项功能**必须**达到：

| 类别 | 标准 |
|---|---|
| **类型检查** | `tsc --strict` 0 错误；`cargo check` 0 错误；`cargo clippy` 无新增 warning |
| **i18n 完整** | `npm run i18n:check` 0 错误；zh / en / tw 三语键数一致 |
| **构建通过** | `npm run build` 成功；`cargo test` 全绿（Rust 端） |
| **6 主题 × 3 颜色模式** | 新 UI 组件在 18 种组合下视觉一致（手动 QA） |
| **手动 QA 全部 PASS** | 每项至少 5 个真实场景，截图存档 |
| **代码风格** | 无 `TODO` / `FIXME` / `as any` / `@ts-ignore`；无内联主题 CSS；无新 ESLint 配置 |
| **提交消息** | Conventional Commits 中文；`feat(scope): ...`；slim 工作流用 `(slim)` scope |
| **性能** | 列出明确的性能预期（如"500 条下 < 50ms"）并在 QA 中验证 |
| **向后退化** | 设置项默认值保持现有行为；旧数据能正常读取 |
| **用户可见价值** | 每个功能有 1 段用户故事 + 至少 1 个 "wow" 场景 |

---

## 8. 建议执行顺序

按"易 → 难 + 价值高"排序（用户已确认 1-9 + CLI 全部接受）：

```
Phase 0（1 天）
  └─ 死代码清理
Phase 1（1-2 月，5 项）
  1. 二维码生成         [1 天, 开门红]
  2. FTS5 + 模糊 + 正则  [2-3 天, 每日价值]
  3. Quick-Paste 浮层    [1-2 天, 键盘党]
  4. 智能分类 + 过滤芯片  [1 周, 视觉]
  5. 文本转换 + 右键菜单  [1 周, 开发者]
Phase 2（2-3 月，4 项）
  6. 屏幕区域截图       [3-4 天]
  7. 本地 OCR           [1 周/平台]
  8. 导出/导入备份      [3-5 天]
  9. tiez-c CLI         [1-2 周]
     └─ 9.1 tiez-c Agent Skill  [1-2 天, 与 9 同步交付]
```

每完成一项 → commit → 跑 `cargo test` + `npm run build` → 手动 QA → 进入下一项。

---

## 9. 关联文档

| 文档 | 内容 |
|---|---|
| [`AGENTS.md`](./AGENTS.md) | 项目知识库、代码约定、反模式 |
| [`README.md`](./README.md) | 项目说明、Linux 依赖、构建步骤 |
| [`docs/cli.md`](./docs/cli.md) | （待写）tiez-c CLI 用户文档 |
| [`docs/i18n.md`](./docs/i18n.md) | （待写）i18n key 命名规范 |
| [`skills/tiez-c-cli/SKILL.md`](./skills/tiez-c-cli/SKILL.md) | （待写）教 LLM 代理使用 tiez-c 的 OpenCode Skills 规范技能 |

---

**最后更新**：2026-06-16
**新增**：5.4.1 tiez-c Agent Skill（教 LLM 使用 CLI 的技能文件计划）
**下次评审**：完成 Phase 1.1（二维码）后
**责任人**：项目维护者本人（本仓库 fork）
