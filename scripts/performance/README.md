# P2 性能证据工具

Rust 文件是测量调用端：旧版本仅通过临时 `git archive` 导出，并作为 Cargo path dependency 调用；新版本直接调用 Core 和 Platform crate。测量使用独立合成数据库；`--target-dir` 可复用编译缓存，完成后另存只读二进制及其构建凭据。

## 准备与构建凭据

使用 Python 3.11+、本机 Rust、Windows MSVC。`prepare_capture.py` 只允许向指定仓库 `.harness/` 下的新目录写入；目录或证据已存在时不覆盖。`--mode prepare` 只生成源码/manifest/证据；`build` 再构建，`verify` 再执行不保留耗时的功能检查。三个模式都不会进行正式采样。

```powershell
python scripts/performance/prepare_capture.py --repository REPOSITORY --candidate CANDIDATE_CHECKOUT --candidate-revision FROZEN_SHA --output REPOSITORY/.harness/capture-evidence --mode verify --profile debug --target-dir REUSABLE_TARGET --jobs 2
```

正式候选要求干净 checkout 且 HEAD 匹配传入 SHA。准备阶段可用 `--allow-dirty-candidate` 检查未提交的 API；只要使用此选项，生成物就标记 `functional_only=true`、`eligible_for_timing=false`，即使期间候选已提交，采样器也会拒绝使用。工具在构建前后核对候选源码指纹，变化即失败。

`preparation.json` 包含实际引用候选源码每个 Git 文件的 SHA256、集合哈希、HEAD 和脏状态；旧源 archive 哈希及临时 manifest 的 workspace 处理前后哈希；Rust/Cargo 版本、profile、target、相关构建环境、机器标识哈希；Python 工具和 Rust 模板哈希；各适配器的实际源码包、角色、manifest/lock/命令、只读二进制哈希及功能报告哈希。适配器 lock 从对应源码 lock 复制，构建后拒绝出现源 lock 以外的外部包版本。release profile 保持 LTO、单 codegen unit、`opt-level=s`、strip、abort。

`--baseline-revision` 可指定已冻结的 alpha.1 完整 SHA `90ba9960744b6cd57caaaf597f68533a7e41be45`。归档存在根 workspace 时保持原样，使用其 `dzc-core`、根 lock 和真实 Platform 密钥提供器；早期单包基线继续使用 `src-tauri` 及其 lock，仅在没有根 workspace 时补空 workspace 隔离。为兼容已有采样命令，基线适配器名称仍为 `legacy`；它表示比较中的基线角色，实际引用的包由凭据记录，不表示 alpha.1 尚未拆分 Core。此适配支持不构成新的采样或性能通过结论。

适配器显式使用 `rustc --print host-tuple` 的本机目标；继承的 `CARGO_BUILD_TARGET` 仍记录在凭据中，但不能改变正式测量的目标。构建保留完整 Cargo JSON 与 stderr 及哈希，只接受本次成功构建报告的唯一 `compiler-artifact.executable`，并核对 manifest、bin、目标和 profile 目录。冻结副本前后再次核对源文件与副本哈希，绝不回退到缓存根目录中的同名文件。回归覆盖同宿主显式 target、空缓存、旧根目录负控、错误或缺失 artifact；`DZC_TEST_REAL_CARGO=1` 可额外运行 std-only 的真实 Cargo JSON 冒烟。

可重复 `--evidence REPORT.json`，把已有 **完整原始 CLI 样本**、CLI build.json 或功能检查 JSON 原样复制到证据目录并记录哈希；不会只保留 p95 摘要。正式结果应包含旧新两端所有轮次、失败轮次和构建凭据。

## 最小测量包装与功能后置条件

`capture/main.rs` 生成中性合成数据，通过生产 initializer 建立真实 SQLite。seed、数据库复制/打开和后置查询都在计时段外。计时内只调用生产仓库方法或实际 CaptureEngine。两端使用相同 payload、记录数和调用模板，依赖真实 Windows DPAPI；不使用测试 key provider。

候选 Core 若提供非默认 `legacy-bridge`，适配器会显式启用它，以 `legacy` 下的隐藏兼容出口测量仓库边界，同时从公开 `capture` 使用实际引擎。这只影响测量程序，不要求稳定 facade 公开数据库连接。

`capture/measured_engine.rs` 经共享既有仓库与缓存的 `history_service()` 兼容桥取得 `HistoryService`，再调用公开的 `HistoryService::capture_store` 工厂。包装按泛型 `CaptureStore` 委托 `prepare/find_duplicate/contains/contains_image_hash/commit`，不引用私有 store 类型；各方法前后读取 Instant 并累计时长。EventSink 只计数，不复制正文。没有插入额外事务、查询、SQL 模型或加密实现。`engine_total_ms` 包含这些少量计时记账及事件计数开销，`engine_non_store_ms` 也包含包装开销，不把它全部归因于业务引擎。六个固定文本/HTML 场景不构成图片查重性能证据。

六个场景为普通新捕获、普通重复、富文本新捕获、敏感新捕获、继承手工保护的敏感重复、实际 `encrypt_entry_with_conn` 转换。每次使用新复制的数据库。功能断言包括正文与 HTML 原样解密、预览、敏感字段密文、逻辑 FTS 可见性、重复 ID、记录数，以及实际 Engine 只发一次 Stored 元数据事件。转换场景直接比较同一生产方法的入口至提交，不把旧版异步队列或 tag 提前返回当作加密完成。

`verify` 每个场景只执行一次，输出 `verification_only=true`、0 次采样、空 metrics；它不是性能证据。真正采样每场景预热 5 次后保留所有原始时长。

## 正式采样（等待冻结 SHA 与安静窗口）

准备阶段通过后，在相同电源配置、后台无重编译的窗口交错执行 old/new/new/old。1000、10000 行都必须采集；至少 30 次，建议 50 次。每轮新建结果文件，不挑选有利轮次。

```powershell
python scripts/performance/run_capture.py --preparation REPOSITORY/.harness/capture-evidence/preparation.json --variant legacy --rows 1000 --iterations 50 --round A1 --output REPOSITORY/.harness/capture-evidence/legacy-1k-A1.json
python scripts/performance/run_capture.py --preparation REPOSITORY/.harness/capture-evidence/preparation.json --variant engine --rows 1000 --iterations 50 --round B1 --output REPOSITORY/.harness/capture-evidence/engine-1k-B1.json
python scripts/performance/compare_capture.py --baseline REPOSITORY/.harness/capture-evidence/legacy-1k-A1.json --candidate REPOSITORY/.harness/capture-evidence/engine-1k-B1.json --output REPOSITORY/.harness/capture-evidence/comparison-1k.json
```

`run_capture.py` 核对只读二进制哈希和机器标识，把原始报告和构建/源码/工具凭据关联到结果。比较器只对共同的 `deduplicate+commit` 和同一转换方法执行 `p95 <= baseline + max(10%, 20ms)`。它要求六个场景与完整原始样本，重算 p50/p95；拒绝不同机器、profile、工具链、模板、数据规模和短样本；核对功能后置条件；失败退出非零。

## CLI 与 scope 边界

已有 `scripts/sample-history-latency.py`、`sample-write-latency.py` 继续单独采集完整 JSON。两端相同 profile/target、schema 对应的 `connect_fixture_database`、相同 seed、1k/10k 中性历史、1 MiB UTF-8/CRLF `add -`、`add @file`、`tag add ID sensitive`。最终 tag 采样加 `--require-protected-conversion`，并保存全部 samples_ms。旧 CLI tag 只写标签，需明确其正文/preview 未加密且 FTS 仍可命中；不能把旧返回时间冒充完整转换的基线。

本工具的共同门槛是 **真实 store 子路径**。Engine 的 prepare、total、non-store 开销单列。旧 AppHandle pipeline 没有等价的 headless 入口，不能把旧仓库 total 与新引擎 total 直接等价比较。因此输出始终保留 `complete_capture_acceptance=false`。它不覆盖 OS reader、listener 调度、GUI、OCR worker、事件运输、异步队列等待，逻辑 FTS 查询也不证明历史 WAL 的物理擦除。已有 CDP/面板样本与 CLI 冷启动同样不替代这些边界。
