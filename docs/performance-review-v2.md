# 项目性能全面审查报告 v2

**日期**: 2026-05-13
**审查方法**: 自底向上代码走查
**审查范围**: Rust Backend (12 files, ~8.5K LOC) + TypeScript CLI (30+ files, ~22K LOC) + Shared Packages + Build/Config

---

## 依赖关系图 (自底向上)

```
Layer 0 [基础层 - 无内部依赖]
  ├── rust-backend/src/pricing/mod.rs       ← 定价引擎
  ├── ccusage/packages/internal/src/pricing.ts  ← TS 定价引擎
  ├── ccusage/packages/internal/src/format.ts   ← 数字/货币格式化
  └── ccusage/packages/terminal/src/table.ts    ← 终端表格渲染

Layer 1 [数据加载层 - 依赖 Layer 0 定价]
  ├── rust-backend/src/sources/*.rs         ← 7个 Source Adapter (5.2K LOC)
  ├── ccusage/apps/ccusage/src/data-loader.ts ← 主数据加载器 (1.7K LOC)
  └── rust-backend/src/cleanup.rs           ← 文件清理

Layer 2 [缓存层 - 独立]
  └── rust-backend/src/cache.rs             ← UsageCache (TTL+LRU)

Layer 3 [服务层 - 依赖 Layer 1+2]
  ├── rust-backend/src/server.rs            ← HTTP API + 刷新调度
  ├── rust-backend/src/protocol.rs          ← API 类型定义
  └── rust-backend/src/main.rs              ← 入口

Layer 4 [CLI应用层 - 依赖 Layer 0+1]
  ├── ccusage/apps/ccusage/src/commands/    ← 主 CLI 命令
  ├── ccusage/apps/codex/src/               ← Codex CLI
  ├── ccusage/apps/opencode/src/            ← OpenCode CLI
  ├── ccusage/apps/pi/src/                  ← Pi CLI
  ├── ccusage/apps/amp/src/                 ← AMP CLI
  └── ccusage/apps/mcp/src/                 ← MCP Server

Layer 5 [顶层集成]
  ├── index.mjs                             ← MCP Server 入口
  └── server/ + shared/                     ← Server adapter
```

---

## 🔴 Critical (可能导致 OOM、请求超时或数据损坏)

### C-01: TS sortFilesByTimestamp 导致 N×2 全文件扫描

**文件**: `ccusage/apps/ccusage/src/data-loader.ts`
**影响**: `loadDailyUsageData()` 调用 `sortFilesByTimestamp()` 对每个文件做 `getEarliestTimestamp()`（完整文件扫描 + JSON 解析），然后再遍历全部文件做实际加载。同一批文件被读取 2 次。`loadSessionBlockData()` 和 `loadSessionData()` 各自再次独立 glob + sortFilesByTimestamp，导致**同一批文件被扫描 4-6 次**。

**修复**:
```typescript
// 方案1: 基于文件 mtime 排序（O(1) per file）
// 方案2: 单次扫描同时排序+加载
// 方案3: 只在首次 glob 时提取 timestamp，后续复用
```

### C-02: Rust 多个 view 独立加载全量文件

**文件**: `rust-backend/src/sources/claude.rs`, `codex.rs` 等
**影响**: 每次请求 `daily`、`monthly`、`sessions` view 时，各自独立调用 `load_usage_entries()` → 同一批 JSONL 文件被完整读取和 JSON 解析 3 次。对于 10K+ 文件、百万级条目的场景，I/O 放大 3 倍。

**修复**: 在 source adapter 级别实现一次性加载 entries → 多视图共享同一份内存数据。

### C-03: TS monthly/weekly 通过 daily 全量 pipeline 再聚合

**文件**: `ccusage/apps/ccusage/src/data-loader.ts`
**影响**: `loadMonthlyUsageData()` 调用 `loadBucketUsageData()` → 内部调用 `loadDailyUsageData()` → 完整文件加载 + JSON 解析 + daily 聚合 → 然后再做一次 monthly 聚合。对百万级条目，计算量翻倍。

**修复**: 直接从 entries 按月份分组聚合，跳过 daily 中间层。

### C-04: Rust SQLite 全表扫描 + json_extract 无索引

**文件**: `rust-backend/src/sources/opencode.rs`
**影响**: `append_sqlite_daily_entries()` 的 SQL 含 10+ COALESCE + 多字段 json_extract，SQLite 无法利用 B-tree 索引。百万级消息时单次查询可能耗时数秒至数十秒。虽然 `try_prepare_opencode_indexes()` 创建了 `time_created` 索引，但 `json_extract(data, '$.xxx')` 仍然需要逐行解析 JSON。

**修复**:
1. 对 `time_created` 的 WHERE 过滤可以利用索引（已有 `>= ?1 AND time_created < ?2` 路径）
2. 考虑提取关键 JSON 字段到独立列（应用层或在数据写入时）
3. 或引入预聚合表

---

## 🟠 High (显著影响响应延迟和吞吐量)

### H-01: Rust UsageCache has()/get() 使用写锁而非读锁

**文件**: `rust-backend/src/cache.rs`
**影响**: `has()` 和 `get()` 每次调用都获取 `data.write().await`（写锁），然后在锁内更新 `entry.last_accessed`。由于使用了 `tokio::sync::RwLock`，写锁是排他锁 — 所有并发读请求串行化。在高并发 API 请求下，缓存访问成为瓶颈。

**修复**:
```rust
pub async fn has(&self, key: &str) -> bool {
    let mut data = self.data.read().await; // 改为读锁
    // last_accessed 更新可延迟或异步批量更新
    data.contains_key(key)
}
```

### H-02: Rust std::sync::RwLock 在 async 上下文中阻塞线程

**文件**: `rust-backend/src/pricing/mod.rs:54`
**影响**: `static PRICING: OnceLock<RwLock<PricingDataset>>` 使用 `std::sync::RwLock`。在 async 运行时中持有 `std::sync` 锁会阻塞 OS 线程，导致该线程上的所有其他任务停滞。

**修复**: 替换为 `tokio::sync::RwLock`。

### H-03: Rust reqwest::blocking 阻塞 tokio worker 线程

**文件**: `rust-backend/src/pricing/mod.rs`
**影响**: `fetch_pricing_map()` 使用 `reqwest::blocking::Client`，15秒超时期间阻塞整个 tokio worker 线程。

**修复**: 使用异步 `reqwest::Client` 或 `tokio::task::spawn_blocking` 隔离。

### H-04: Rust candidates_for() 每次分配 200+ 字符串

**文件**: `rust-backend/src/pricing/mod.rs`
**影响**: 每次 `model_cost_usd()` 调用生成 20+ prefix × 4 normalize = 200+ 候选字符串。对于百万级条目，产生 2 亿+ 字符串分配。

**修复**: 添加 `HashMap<String, Vec<String>>` 缓存 candidates_for 结果。

### H-05: TS sharedPricingMapPromises Promise 泄漏

**文件**: `ccusage/packages/internal/src/pricing.ts`
**影响**: 全局 `sharedPricingMapPromises` Map 在 promise resolve 后不被移除，长期运行的进程（如 MCP server）内存持续增长。

**当前代码**: `promise.then(() => { ... delete ... }, () => { ... delete ... })` — 已修复 ✅（当前代码有 then/catch 双向清理）

**验证**: 当前代码已正确处理成功和失败路径的清理。此项风险已降低。

### H-06: TS calculateCostFromTokens 三重冗余 fallback

**文件**: `ccusage/packages/internal/src/pricing.ts`
**影响**: 模型定价查找触发三个独立 fetch 链（litellm → llm-prices → local cache），模型名不存在时造成 3× 网络延迟。

**修复**: 实现短路逻辑，litellm 覆盖多数模型时仅在缺失时 fallback。

### H-07: Rust today_summary() 双重模型名称收集

**文件**: `rust-backend/src/server.rs`
**影响**: `collect_model_names()` 从 `modelsUsed` 字段提取，`collect_model_totals()` 从 `modelBreakdowns` 字段提取，然后再次 `source_models.extend(model_totals.keys())` 合并 — 同一组模型名被处理 3 次。

**修复**: 单次遍历，从 modelBreakdowns 同时提取 names 和 totals。

### H-08: Rust server 启动时加载数据两遍

**文件**: `rust-backend/src/server.rs`
**影响**: `refresh_startup()` 加载所有 source 的 daily → 后续 `refresh_all()` 再加载所有 source × view。启动时同一数据被加载 2 轮。

**修复**: 合并两次加载，先加载 raw entries → 再派生所有视图。

### H-09: Rust refresh_daily_range 逐天请求无批量

**文件**: `rust-backend/src/server.rs`
**影响**: 逐天调用 `provider.load_today_daily()`，每天内部独立打开/读取文件。N 天 = N 次文件系统扫描。

**修复**: 改为单次全量加载 → 按日期过滤分发。

---

## 🟡 Medium (累积效应下影响吞吐量)

### M-01: TS Intl.NumberFormat/DateTimeFormat 每次 new

**文件**: `ccusage/packages/terminal/src/table.ts`
**影响**: `formatNumber()` 使用 `num.toLocaleString('en-US')` — 每次调用都创建新的 formatter。

**当前代码**: `format.ts` 已使用模块级 `tokenFormatter` 和 `currencyFormatters` Map ✅
**但**: `table.ts` 中的 `formatNumber()` 仍用 `toLocaleString()` 而非复用 formatter。

**修复**: 将 `formatNumber()` 改为使用模块级 formatter。

### M-02: Rust prune_expired() 每次 O(n) 全量扫描

**文件**: `rust-backend/src/cache.rs`
**影响**: `has()`, `get()`, `health()` 每次都调用 `prune_expired()` 遍历整个 HashMap。当缓存接近上限时，每次 API 请求都做 O(500) 扫描。

**修复**: 使用延迟清理策略（仅当缓存满时触发）或定时后台清理。

### M-03: Rust replace_rows_by_string_field() Arc::make_mut() 深拷贝

**文件**: `rust-backend/src/cache.rs`
**影响**: `Arc::make_mut()` 在存在其他引用时触发深拷贝。如果 `today_cache` 同时持有同一 `Arc<Vec<Value>>` 的引用，此操作会完整复制所有 rows。

**修复**: 在调用前清除 today_cache 中对应 key，或使用 `Arc::unwrap_or_clone()` 替代。

### M-04: Rust enforce_today_cache_limit() 线性查找

**文件**: `rust-backend/src/server.rs`
**影响**: 使用 `cache.keys().min_by_key()` 线性扫描所有 key 找最老日期。

**修复**: 使用 `BTreeMap<String, ...>` 替代 HashMap，O(log n) 查找最小 key。

### M-05: Rust BTreeMap/BTreeSet 代替 HashMap

**文件**: `rust-backend/src/server.rs`
**影响**: `model_totals: BTreeMap`, `all_models: BTreeSet`, `source_models: BTreeSet` — B-Tree 比 HashMap 慢 2-3x（红黑树 vs 散列表）。当前数据量小（<100 models），影响有限，但原理上应使用 HashMap。

**修复**: 替换为 HashMap + 最终排序（仅在输出前排序一次）。

### M-06: TS valibot 每行 JSON 完整 schema 验证

**文件**: `ccusage/apps/ccusage/src/data-loader.ts`
**影响**: 对每条 JSONL 行做 `v.safeParse(usageDataSchema, parsed)` 完整验证。百万级行时，valibot 验证成为 CPU 热点。

**修复**: 热路径中跳过验证，使用 `JSON.parse` + 可选字段提取。

### M-07: Rust server filter_by_date_range() 线性扫描

**文件**: `rust-backend/src/server.rs`
**影响**: 每次带 `since/until` 参数的请求都线性扫描所有 cached rows。

**修复**: 缓存中使用 `BTreeMap<DateString, Vec<Value>>` 索引，O(log n) 范围查询。

### M-08: Rust 7 个 adapter 重复实现聚合逻辑

**文件**: `rust-backend/src/sources/*.rs`
**影响**: `entries_to_daily()`, `entries_to_monthly()`, `entries_to_sessions()` 在多个 adapter 中几乎相同实现（~5 次重复）。

**修复**: 抽取公共 trait/函数，adapter 仅提供差异化的数据提取。

### M-09: TS 6 个 CLI app 重复 data-loader 和 schema

**文件**: `ccusage/apps/*/data-loader.ts`
**影响**: 每个 app 独立实现文件扫描、定价、schema 定义。

**修复**: 提升到 `packages/internal`，各 app 保留数据源适配差异。

### M-10: SQLite 无连接池

**文件**: `rust-backend/src/sources/opencode.rs`
**影响**: 每次 `load_source_view()` 创建新 `Connection::open_with_flags()`。

**修复**: 使用单例连接 + Mutex 或连接池。

### M-11: Rust server 无请求频率限制

**文件**: `rust-backend/src/server.rs`
**影响**: 无 rate limiting，错误客户端可能触发大量重复刷新。

**修复**: 添加 `tower::limit::RateLimitLayer`。

### M-12: Rust server 大响应体无压缩

**文件**: `rust-backend/src/server.rs`
**影响**: 已有 `CompressionLayer::new()` ✅ — 此项已修复。

---

## 🟢 Low (边际改善)

### L-01: Rust unresolved_models HashSet 只增不减

**文件**: `rust-backend/src/pricing/mod.rs`
**影响**: 未知模型名累积，但数量可控（<1000）。

### L-02: Rust Err(_) => continue 静默跳过

**文件**: `rust-backend/src/sources/*.rs`
**影响**: 无日志，难以排查。添加 `tracing::warn!` 即可。

### L-03: TS catch { continue } 静默跳过

**文件**: `ccusage/apps/ccusage/src/data-loader.ts`
**影响**: 无法追踪跳过原因。

### L-04: Rust cleanup.rs 递归扫描目录

**文件**: `rust-backend/src/cleanup.rs`
**影响**: 清理频率低，递归开销可接受。

### L-05: 构建脚本无并行化

**文件**: `scripts/build-native-mac.mjs`
**影响**: Rust build + Swift build 串行执行，总构建时间 ~5-10 分钟。

---

## 优化优先级矩阵

| 优先级 | 编号 | 问题 | 预期收益 | 改动量 | 风险 |
|--------|------|------|----------|--------|------|
| **P0** | C-01 | TS sortFilesByTimestamp N×2 全文件扫描 | 减少 3-5× I/O | 小 | 低 |
| **P0** | C-02 | Rust 多 view 重复加载文件 | 消除 2-3× 重复 I/O | 中 | 中 |
| **P0** | H-01 | UsageCache 写锁阻塞并发读取 | 高并发下 5-10× 加速 | 小 | 低 |
| **P0** | H-02 | std::sync::RwLock 阻塞 async | 消除线程停滞 | 小 | 低 |
| **P1** | C-03 | TS monthly/weekly 重复全量计算 | 消除 2× 重复计算 | 中 | 中 |
| **P1** | C-04 | SQLite json_extract 全表扫描 | 查询加速 10-100× | 中 | 中 |
| **P1** | H-03 | reqwest::blocking 阻塞 tokio | 消除 15s 线程阻塞 | 小 | 低 |
| **P1** | H-04 | candidates_for 200+ 字符串分配 | 减少 2 亿+ 分配 | 小 | 低 |
| **P1** | H-08 | 启动加载数据两遍 | 启动时间减半 | 中 | 低 |
| **P1** | M-06 | valibot 每行验证 | CPU 减少 20-30% | 小 | 中 |
| **P2** | H-07 | today_summary 双重模型收集 | 减少 50% 模型处理 | 小 | 低 |
| **P2** | H-09 | 逐天刷新无批量 | 减少 N× 文件扫描 | 中 | 低 |
| **P2** | M-02 | prune_expired 每次 O(n) | 减少热路径开销 | 小 | 低 |
| **P2** | M-05 | BTreeMap → HashMap | 2-3× 查找加速 | 小 | 低 |
| **P2** | M-07 | filter_by_date_range 线性扫描 | O(log n) 范围查询 | 中 | 低 |
| **P3** | M-01 | Intl formatter 复用 | 5-10× 格式化加速 | 小 | 低 |
| **P3** | M-10 | SQLite 连接池 | 减少连接开销 | 小 | 低 |
| **P3** | M-11 | 请求频率限制 | 防止滥用 | 小 | 低 |
| **P3** | L-01~L-05 | 低优先级项 | 边际改善 | 小 | 低 |

---

## 架构级建议

### 1. 统一数据加载层

```
┌──────────────────────────────────┐
│       API / CLI / MCP             │  ← 统一查询接口
├──────────────────────────────────┤
│    统一缓存层 (TTL + LRU)         │  ← 共享数据缓存
├──────────────────────────────────┤
│    统一数据加载层                 │  ← 一次性加载 + 增量更新
├──────────────────────────────────┤
│  Source Adapters (最小化差异)      │  ← 只做数据提取
└──────────────────────────────────┘
```

**核心原则**: 每次请求不应重新扫描所有历史文件。

### 2. 增量加载

- Rust: 基于文件 mtime 和缓存状态判断是否需要重新读取
- TS: 引入本地 SQLite 缓存层，仅文件变更时增量更新
- 所有层: 日期范围过滤作为第一道防线

### 3. 缓存策略标准化

| 缓存类型 | TTL | 淘汰 | 场景 |
|----------|-----|------|------|
| 请求级 | 请求结束 | N/A | 单次请求内多视图 |
| 短期 | 5-30 min | LRU (500) | 定价数据、文件扫描结果 |
| 长期 | 跨天 | 日期边界淘汰 | 聚合后的 daily/monthly |

### 4. 批量操作优先

- 定价: 收集唯一模型名 → 批量查询 → 映射回条目
- 聚合: 一次性加载 raw entries → 内存派生所有视图
- I/O: 合并相邻日期范围扫描为单次操作

---

## 已验证的改进 (相比前版审计)

| 之前发现 | 当前状态 | 变化 |
|----------|----------|------|
| UsageCache 无 TTL/容量 | 已有 TTL(30min) + max(500) + LRU | ✅ 已修复 |
| server 无压缩 | 已有 CompressionLayer | ✅ 已修复 |
| sharedPricingMapPromises 泄漏 | 已有 then/catch 双向清理 | ✅ 已修复 |
| format.ts Intl 复用 | 已有模块级 formatter | ✅ 已修复 |
| SQLite 索引 | 已有 try_prepare_opencode_indexes | ✅ 部分修复 |
| cleanup.rs 未删除文件 | 已有 Apply 模式执行删除 | ✅ 已修复 |

**新增发现的问题**:
- UsageCache has()/get() 使用写锁而非读锁（新发现）
- TS data-loader sortFilesByTimestamp N×2 全文件扫描（新发现详细量化）
- BTreeMap/BTreeSet 在 server.rs 中的不必要使用（新发现）
- Arc::make_mut() 深拷贝风险（新发现）
- valibot 每行验证的 CPU 开销（新发现量化）
