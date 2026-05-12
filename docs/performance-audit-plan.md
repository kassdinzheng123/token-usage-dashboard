# 性能审查计划 (Performance Audit Plan)

**目标**: 自底向上检查项目中潜在的性能问题，重点关注内存泄漏、重复计算、Heavy Load 等情况。

**项目概览**:
- **Rust Backend** (~7.8K lines): HTTP 服务器，聚合 7 种 AI 工具的 token 用量数据（Claude, Codex, OpenCode, Pi, Factory, Hermes, Openclaw）
- **TypeScript CLI Apps** (~22K lines): `ccusage/apps/` 下 6 个独立 CLI 应用，各自处理不同工具的使用数据
- **共享包** (`ccusage/packages/`): 通用定价和数据加载逻辑

---

## 阶段 1: 数据加载层 (Data Loading Layer)

### 1.1 Rust 源适配器的文件 I/O 扫描

**文件**:
- `rust-backend/src/sources/claude.rs` — `collect_jsonl_files()`, `collect_jsonl_files_modified_since()`, `read_usage_file()`
- `rust-backend/src/sources/codex.rs` — `collect_jsonl_files()`
- `rust-backend/src/sources/opencode.rs` — `append_json_messages()`, `collect_json_files()`
- `rust-backend/src/sources/factory.rs` — `collect_settings_files()`, `read_jsonl_metadata()`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 1.1.1 | 递归 `fs::read_dir` 无深度/文件数量限制 | 中 | 大量项目时扫描可能极慢（数千 JSONL 文件） |
| 1.1.2 | `read_usage_file()` 逐行解析 + JSON 反序列化 + 去重 HashSet 全部在内存 | 高 | 大文件（>100MB）JSONL 可能导致内存飙升 |
| 1.1.3 | 多个 view（daily/monthly/sessions）各自调用 `load_usage_entries()`，**同一批文件被多次读取和解析** | 高 | **严重重复 I/O + 重复 JSON 解析** |
| 1.1.4 | `load_usage_entries()` vs `load_usage_entries_for_date()` 之间逻辑重复，后者额外 `collect_jsonl_files_modified_since()` 做 mtime 过滤但仍全文件扫描 | 中 | mtime 过滤不可靠（文件修改时间 ≠ 内容时间） |
| 1.1.5 | `factory.rs` 的 `read_jsonl_metadata()` 打开 JSONL 文件只为找第一条有 timestamp 的行 — 无边界 | 低 | 坏文件可能读完整文件 |

### 1.2 TypeScript 数据加载

**文件**:
- `ccusage/apps/ccusage/src/data-loader.ts` — `loadDailyUsageData()`, `loadSessionData()`, `sortFilesByTimestamp()`, `getEarliestTimestamp()`
- 其他 apps 的 `data-loader.ts`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 1.2.1 | `getEarliestTimestamp()` 逐文件完整扫描以获取最早时间戳；`sortFilesByTimestamp()` 对每个文件调用一次 | **高** | N 个文件 → N 次完整文件扫描 + N 次 JSON 解析，然后再真正加载时又做 N 次 |
| 1.2.2 | `loadDailyUsageData()` 将所有条目收集到 `allEntries` 数组再分组 — 大量条目时内存占用大 | 中 | 对百万级条目，`allEntries` 可能占用数百 MB |
| 1.2.3 | `processedHashes: Set<string>` 随条目数线性增长 — 大负载下无界 | 中 | 处理完不会被释放（直到函数返回） |
| 1.2.4 | `calculateContextTokens()` 使用 `readFile()` 一次性读入整个 transcript 文件再 `split('\n').reverse()` | 高 | 大 transcript 文件（GB 级）会导致内存爆炸 |
| 1.2.5 | `loadBucketUsageData()` 内部调用 `loadDailyUsageData()` 再二次聚合 — 即 daily 数据被算两次 | 中 | 调用 weekly/monthly 时，daily 数据全量计算后再分组 |
| 1.2.6 | `loadSessionBlockData()` 重新扫描所有文件 + 重新解析 — 与 daily 逻辑高度重复 | 中 | 同批数据被加载 2-3 次 |

---

## 阶段 2: 缓存与内存管理 (Caching & Memory)

### 2.1 Rust 后端 `UsageCache`

**文件**: `rust-backend/src/cache.rs`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 2.1.1 | `data: RwLock<HashMap<String, Arc<Vec<Value>>>>` 无任何过期策略或最大容量限制 | **高** | 随着 source/view 增多且数据量增长，缓存无限膨胀 |
| 2.1.2 | `errors: RwLock<HashMap<String, String>>` 仅 `remove` 在成功写入时 — 失败后错误字符串永存 | 低 | 小问题 |
| 2.1.3 | `today_cache: HashMap<(Source, String), Arc<Vec<Value>>>` 在 `refresh_all()` 中 `clear()` — 但单点刷新不会清理旧日期的缓存 | 中 | 日期跨天后旧数据仍驻留 |
| 2.1.4 | `RwLock` 在热路径频繁读写 — 高并发请求下可能成为瓶颈 | 低 | 当前是单实例，实际影响有限 |

### 2.2 Rust 定价缓存

**文件**: `rust-backend/src/pricing/mod.rs`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 2.2.1 | `static PRICING: OnceLock<RwLock<PricingDataset>>` — 全局静态，包含两个 `HashMap` (primary + secondary) | 低 | 数据量可控（数千模型），非泄漏 |
| 2.2.2 | `unresolved_models: HashSet<String>` 只增不减 — 未知模型名累积 | 低 | 除非大量一次性模型，否则无害 |
| 2.2.3 | `candidates_for()` 为单个模型生成 **20+ 个 prefix 变体** × 4 种 normalize 变体 ≈ **200+ 个候选字符串**，全部分配 | **中** | 每次 `model_cost_usd()` 调用都触发大量 String 分配和 HashSet 操作 |
| 2.2.4 | `add_candidate()` 为每个候选生成 4 个 normalize 变体（替换 `:` `.` 为 `-`） | 低 | 小开销，但频次高 |

### 2.3 TypeScript 定价与全局状态

**文件**: `ccusage/packages/internal/src/pricing.ts`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 2.3.1 | `sharedPricingMapPromises: Map<string, Promise<Map>>` 模块级全局 — promise resolve 后会持续占用内存直到 `catch` 清理 | **中** | 成功解析后 promise 不被移除 — **潜在的 promise 泄漏** |
| 2.3.2 | `LiteLLMPricingFetcher` 使用 `Disposable`，但 `clearCache()` 仅清内部字段，不清 `sharedPricingMapPromises` | 中 | 多个 fetcher 实例共享同一全局 promise map |
| 2.3.3 | `createMatchingCandidates()` 与 Rust 的 `candidates_for()` 类似 — 大量字符串分配 | 低 | JS 垃圾回收通常能处理 |

---

## 阶段 3: 重复计算 (Redundant Computations)

### 3.1 Rust 后端

**文件**: `rust-backend/src/server.rs`, `rust-backend/src/sources/*.rs`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 3.1.1 | **每个 view 请求独立加载全量数据**: `load_source_view("daily")` 和 `load_source_view("monthly")` 各自读取全量文件并重新聚合 | **高** | 若请求了 daily 和 monthly 两次，相同 JSONL 文件被读两次、解析两次、聚合两次 |
| 3.1.2 | `today_summary()` 并行加载所有 source 的 daily，然后对每个 row 做 `collect_model_names()` + `collect_model_totals()` — 如果已有 `modelBreakdowns` 字段则多余 | 中 | 双重 model 名称收集 |
| 3.1.3 | `refresh_startup()` 加载所有 daily → `refresh_all()` 再加载所有 source × view → 启动时同一数据被加载 2 轮 | 中 | 启动开销大 |
| 3.1.4 | `refresh_daily_range()` 中 `provider.load_today_daily()` 逐天请求，但每天内部又重新读文件 — 无批量优化 | 中 | N 天 = N 次文件扫描（非 SQLite 路径） |
| 3.1.5 | `opencode.rs` 的 SQLite 路径与 JSON 路径逻辑重复 — `load_daily_from_sqlite()` 和 `load_usage_entries_between()` 做类似聚合 | 低 | 代码复用，但逻辑正确 |

### 3.2 TypeScript CLI

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 3.2.1 | `loadMonthlyUsageData()` 调用 `loadBucketUsageData()` → 内部调用 `loadDailyUsageData()` — 即 **完整文件加载 + 聚合 → 再聚合** | **高** | 请求 monthly 时，daily 数据完整计算一遍再聚合 |
| 3.2.2 | `loadWeeklyUsageData()` 同上 — 完整的 daily pipeline 跑一次只为做 weekly 分组 | 高 | 同上 |
| 3.2.3 | 每个 CLI app (`ccusage`, `codex`, `opencode`, `amp`, `pi`) 有各自的 `data-loader.ts`，文件扫描逻辑基本复制粘贴 | 低 | 维护性问题，非性能问题 |
| 3.2.4 | `aggregateByModel()` 被多次调用 — `calculateTotals()` + `aggregateByModel()` 各自遍历同一批 entries | 中 | 同一数组被遍历 3 次以上 |

---

## 阶段 4: Heavy Load 场景 (高负载)

### 4.1 并发与线程

**文件**: `rust-backend/src/server.rs`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 4.1.1 | `REFRESH_CONCURRENCY = 4` — 刷新任务限制为 4 并发，但每个任务 `spawn_blocking` 占用 OS 线程 | 中 | 若每个源扫描耗时 5s，22 个 task 需 ~27s |
| 4.1.2 | `tokio::task::spawn_blocking` 无池大小限制 — 大量并发请求时可能耗尽 OS 线程 | 中 | axum 默认的 blocking pool 为 50 线程 |
| 4.1.3 | `refresh_with_lock()` 使用 per-key `Mutex` 防止重复刷新，但 `today_cache` 的锁粒度更粗（整个 HashMap 一把锁） | 低 | 同一 key 并发请求会排队 |
| 4.1.4 | `today_summary()` 对每个 source `tokio::spawn` — 如果 source 数量增长或响应慢，handle 累积 | 低 | 当前只有 7 个 source |

### 4.2 SQLite 连接管理

**文件**: `rust-backend/src/sources/opencode.rs`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 4.2.1 | 每次 `load_source_view()` 创建新的 `Connection::open_with_flags()` — 无连接池 | 中 | 高频请求下反复打开/关闭 SQLite |
| 4.2.2 | `append_sqlite_messages()` 使用 `SELECT ... FROM message` 无 WHERE 条件 — 全表扫描 | 中 | 大量消息时性能差 |
| 4.2.3 | `append_sqlite_daily_entries()` 的 SQL 查询包含 10 个 `COALESCE` + 多字段 `json_extract` — 无索引辅助 | 高 | SQLite `json_extract` 无法利用索引，全表扫描 + 逐行 JSON 解析 |

### 4.3 网络请求

**文件**: `rust-backend/src/pricing/mod.rs`, `ccusage/packages/internal/src/pricing.ts`

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 4.3.1 | Rust 定价模块使用 `reqwest::blocking::Client` — 阻塞线程池做 HTTP 请求 | 低 | 有 15s 超时保护，但阻塞线程是浪费 |
| 4.3.2 | TypeScript 的 `sharedPricingMap()` 直接 `fetch()` 无重试/熔断 | 低 | 网络抖动时可能反复失败 |
| 4.3.3 | 两个 HTTP 源 (litellm + llm-prices) 各下载一个大型 JSON — litellm 文件可能 >2MB | 低 | 首次加载可能较慢 |

---

## 阶段 5: 代码结构与可维护性

### 5.1 重复代码模式

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 5.1.1 | 7 个 source adapter 各自实现类似的 `entries_to_daily()`, `entries_to_monthly()`, `entries_to_sessions()` — 聚合逻辑高度重复 | 低（性能）/ 中（维护） | 相同聚合逻辑重复 ~5 次 |
| 5.1.2 | TypeScript 的 6 个 CLI app 各自有 `data-loader.ts`, `pricing.ts` 等 — 大量重复的 schema 定义和聚合逻辑 | 低（性能）/ 中（维护） | 通过 `ccusage/packages/internal` 部分共享但各 app 仍有独立实现 |
| 5.1.3 | `collect_jsonl_files()` / `collect_json_files()` 在多个 source 中复制 | 低 | 小函数重复 |

### 5.2 错误处理

**检查项**:
| # | 检查点 | 风险 | 说明 |
|---|--------|------|------|
| 5.2.1 | Rust source 模块中 `Err(_)` → `continue` 静默跳过 — 无日志 | 低 | 难以排查个别文件解析失败 |
| 5.2.2 | TypeScript 中 `catch { continue }` 同样静默 — 无法追踪跳过原因 | 低 | 同上 |

---

## 审查执行计划

### 优先级排序 (P0 → P2)

| 优先级 | 编号 | 问题 | 影响 |
|--------|------|------|------|
| **P0** | 1.1.3 | Rust: 同一批 JSONL 文件为不同 view 重复读取和解析 | 3x I/O + 解析开销 |
| **P0** | 1.2.1 | TS: `sortFilesByTimestamp` 导致全文件扫描 N 次仅为了排序 | N 倍 I/O 浪费 |
| **P0** | 3.2.1/3.2.2 | TS: monthly/weekly 通过 daily 全量 pipeline 获取数据 | 2x 全量计算浪费 |
| **P0** | 2.1.1 | Rust: `UsageCache` 无过期/容量限制 | 长期运行的内存持续增长 |
| **P1** | 4.2.3 | Rust: SQLite `json_extract` 全表扫描 | 大量消息数据时性能差 |
| **P1** | 2.3.1 | TS: `sharedPricingMapPromises` 不释放成功 promise | 长期运行的 promise 泄漏 |
| **P1** | 2.2.3 | Rust: `candidates_for()` 每次生成 200+ 候选字符串 | 高调用频次下的 CPU/内存浪费 |
| **P1** | 1.2.4 | TS: `calculateContextTokens()` 一次性读入整个 transcript | 大文件 OOM 风险 |
| **P2** | 4.1.1 | Rust: `spawn_blocking` 启动开销 | 单次刷新延迟 |
| **P2** | 3.1.3 | Rust: startup 加载数据两遍 | 启动慢但仅一次 |
| **P2** | 4.2.1 | Rust: SQLite 无连接池 | 高频请求下连接开销 |

### 审查方法

1. **代码走查** (已完成初步分析) — 逐文件检查上述检查点
2. **负载测试** — 准备大体积 JSONL 文件（>100MB, >100K entries）测试实际性能
3. **内存 profiling** — 使用 `heaptrack`/`dhat` (Rust) 和 `--inspect` (Node) 检测内存增长
4. **SQLite 分析** — 使用 `EXPLAIN QUERY PLAN` 确认查询是否走索引

### 建议的优化方向

| 方向 | 预期收益 | 改动量 |
|------|----------|--------|
| Rust: 预加载 entries 到内存，多 view 共享同一份数据 | 消除 2-3x 重复 I/O | 中 |
| Rust: UsageCache 添加 TTL 或 LRU 策略 | 防止内存无限增长 | 小 |
| TS: `sortFilesByTimestamp` 改为基于 `mtime` 或文件头快速扫描 | 减少 N 倍全文件扫描 | 小 |
| TS: monthly/weekly 直接从 entries 聚合，跳过 daily 中间层 | 消除多余聚合步骤 | 中 |
| TS: `calculateContextTokens` 改为流式读取 | 防止大文件 OOM | 小 |
| TS: `sharedPricingMapPromises` resolve 后清理 | 防止 promise 泄漏 | 小 |
| Rust: SQLite 查询添加索引或改为预聚合表 | 大幅加速查询 | 中 |
| Rust: `candidates_for()` 缓存结果 | 减少重复字符串分配 | 小 |
