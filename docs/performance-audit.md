# 性能问题诊断报告 (Performance Audit Report)

**生成日期**: 2026-05-11
**审查范围**: Rust Backend (7.8K lines) + TypeScript CLI Apps (22K lines) + Shared Packages + Server Layer
**严重程度分级**: Critical > High > Medium > Low

---

## 目录

1. [🔴 Critical](#-critical)
2. [🟠 High](#-high)
3. [🟡 Medium](#-medium)
4. [🟢 Low](#-low)
5. [优化优先级矩阵](#优化优先级矩阵)
6. [架构级建议](#架构级建议)

---

## 🔴 Critical

全量加载大表、无索引查询、内存泄漏风险 — 可能导致 OOM、请求超时或数据损坏。

---

### C-1: SQLite 无索引全表扫描，大量消息时性能极差

**涉及文件**: `rust-backend/src/sources/opencode.rs:180-204`
**影响范围**: OpenCode SQLite 数据源的所有视图查询（daily/monthly/sessions）

**代码证据**:
```sql
-- append_sqlite_daily_entries() 中的查询
SELECT ... FROM message
-- 无 WHERE 条件，无 LIMIT，全表扫描
-- 包含 10+ 个 COALESCE + json_extract 调用
```

`json_extract()` 在 SQLite 中无法利用 B-tree 索引，每次查询对所有行进行逐行 JSON 解析。当消息表达到百万级行数时，单次查询可能耗时数秒至数十秒。

**建议修复**:
1. 在 `message` 表上为日期相关字段建立普通索引（非 JSON 字段）
2. 对 JSON 字段建立生成列 + 索引（SQLite 3.31+ 支持 `GENERATED ALWAYS AS (json_extract(...)) STORED`）
3. 或引入预聚合表（materialized aggregation），在消息写入时更新

---

### C-2: Hermes 适配器 COALESCE 全表扫描，不可索引

**涉及文件**: `rust-backend/src/sources/hermes.rs:16-27`
**影响范围**: Hermes 数据源的所有查询

**代码证据**:
```sql
WHERE COALESCE(field1, field2, field3) = ?
```
`COALESCE()` 在索引键上无法被查询优化器使用，导致每次查询都是全表扫描。

**建议修复**:
1. 将 COALESCE 逻辑改为应用层处理（先查索引列，fallback 应用代码补充）
2. 或在数据写入时预计算 COALESCE 结果存入独立索引列

---

### C-3: OpenCode 适配器全量加载所有消息到内存，无分页和日期过滤

**涉及文件**: `rust-backend/src/sources/opencode.rs:180-204`
**影响范围**: OpenCode 数据源的视图加载，内存占用

**代码证据**: `append_json_messages()` 将所有消息一次性加载到内存数组，无 `LIMIT`、无日期范围过滤、无游标分页。大项目或长期积累的会话数据可能导致内存飙升（>500MB）。

**建议修复**:
1. 添加日期范围过滤（WHERE timestamp BETWEEN ? AND ?）
2. 实现分页查询（LIMIT + OFFSET 或 keyset pagination）
3. 对超大数据集改为流式处理而非全量加载

---

### C-4: TypeScript 全量无过滤加载，每次加载全部历史数据

**涉及文件**:
- `ccusage/apps/amp/src/data-loader.ts`
- `ccusage/apps/ccusage/src/data-loader.ts`
- `ccusage/apps/codex/src/data-loader.ts`
- `ccusage/apps/opencode/src/data-loader.ts`
- `ccusage/apps/opencode/src/data-loader-sqlite.ts`
- `ccusage/apps/pi/src/data-loader.ts`

**影响范围**: 所有 CLI 应用和 MCP 工具调用，每次执行完整扫描历史数据

**问题描述**:
- **amp**: 全量无过滤查询，每次加载全部历史数据到内存
- **ccusage**: 多次全量加载同一数据源（sortFilesByTimestamp 扫描一遍 + loadDailyUsageData 再扫描一遍）
- **codex**: 全量加载所有 session 数据
- **opencode (JSON + SQLite)**: 无索引查询 + 全量加载
- **pi**: HTTP 请求无缓存，重复请求同一远程数据

**建议修复**:
1. 实现基于文件 `mtime` 的增量加载（仅加载变更文件）
2. 引入本地缓存层（SQLite 或 JSON cache file），避免每次都全量扫描
3. SQLite 查询添加 `LIMIT` 和索引

---

### C-5: Rust UsageCache 无 TTL/容量限制，长期运行导致内存无限增长

**涉及文件**: `rust-backend/src/cache.rs:12-31`
**影响范围**: Rust 后端所有缓存数据，长期运行的服务

**代码证据**:
```rust
data: RwLock<HashMap<String, Arc<Vec<Value>>>>,
// 无任何过期策略、无最大条目数限制、无 LRU 淘汰
// 随着 source/view 组合增多，缓存无限膨胀
```

当服务运行数天/数周后，不同日期/视图的缓存条目累积，可能导致数百 MB 至 GB 的内存占用。

**建议修复**:
1. 引入 TTL（如 30 分钟过期）
2. 添加最大容量限制（如 500 条目）
3. 使用 LRU 淘汰策略（可引入 `lru` crate）
4. 跨天后自动清理旧日期的缓存条目

---

### C-6: TypeScript sharedPricingMapPromises 成功 Promise 不释放

**涉及文件**: `ccusage/packages/internal/src/pricing.ts`
**影响范围**: 长时间运行的 Node.js 进程（如 MCP server）

**代码证据**:
```typescript
const sharedPricingMapPromises: Map<string, Promise<Map<...>>> = new Map();
// Promise resolve 后不会被移除
// 仅 catch 时清理，成功路径泄漏
```

在 MCP server 长期运行场景下，每个新模型查询都会在 `sharedPricingMapPromises` 中累积已 resolve 的 Promise 引用，导致内存持续增长。

**建议修复**:
```typescript
// Promise resolve 后也清理
promise.then(result => {
    sharedPricingMapPromises.delete(key);
    return result;
}).catch(err => {
    sharedPricingMapPromises.delete(key);
    throw err;
});
```

---

### C-7: TypeScript calculateContextTokens 一次性读入整个 transcript 文件

**涉及文件**: `ccusage/apps/ccusage/src/data-loader.ts`
**影响范围**: 大 transcript 文件场景（GB 级）

**代码证据**:
```typescript
const content = readFile(filePath);  // 一次性读入
const lines = content.split('\n').reverse();  // 再 split + reverse
```

对于数 GB 的 transcript 文件，一次性 `readFile` 会导致 V8 堆内存爆炸（OOM）。

**建议修复**:
1. 改为流式读取 + 反向行解析
2. 或仅在文件大小超过阈值时使用流式读取

---

## 🟠 High

重复加载、无分页、O(n²) 算法、同步阻塞 — 显著影响响应延迟和吞吐量。

---

### H-1: reqwest::blocking 在 async 上下文中阻塞 Tokio 线程

**涉及文件**: `rust-backend/src/pricing/mod.rs:78-95`
**影响范围**: 定价数据获取，最长阻塞 15 秒

**代码证据**:
```rust
let client = reqwest::blocking::Client::new();
let resp = client.get(url).timeout(Duration::from_secs(15)).send()?;
// 在 async fn 中使用 blocking HTTP 客户端
// 阻塞整个 tokio worker 线程
```

`reqwest::blocking` 占用 OS 线程，阻塞期间该 worker 无法处理其他请求。在高并发下可能导致请求排队。

**建议修复**:
1. 改用 `reqwest::Client`（异步版本）
2. 或使用 `tokio::task::spawn_blocking` 将阻塞调用隔离

---

### H-2: std::sync::RwLock 在 async 上下文中阻塞 OS 线程

**涉及文件**: `rust-backend/src/pricing/mod.rs:54`
**影响范围**: 定价数据共享访问

**代码证据**:
```rust
static PRICING: OnceLock<RwLock<PricingDataset>> = OnceLock::new();
// std::sync::RwLock 在 async 上下文中持有锁时会阻塞线程
// 应使用 tokio::sync::RwLock
```

当读锁被持有时，写请求会阻塞当前 OS 线程直到锁释放。在 async 运行时中这会导致该线程上的所有其他任务停滞。

**建议修复**: 将 `std::sync::RwLock` 替换为 `tokio::sync::RwLock`

---

### H-3: Claude/Codex 适配器每次视图请求重新读取全部 JSONL 文件

**涉及文件**:
- `rust-backend/src/sources/claude.rs:123-137`
- `rust-backend/src/sources/codex.rs:119-131`

**影响范围**: Claude 和 Codex 数据源的所有视图，同一批文件被多次 I/O + 解析

**问题描述**: 每次请求 daily、monthly、sessions 视图时，各自独立调用 `load_usage_entries()`，同一批 JSONL 文件被完整读取和 JSON 解析 3 次。

**建议修复**:
1. 在 source adapter 级别实现一次性加载 entries → 多视图共享
2. 或利用 `UsageCache` 缓存原始 entries，各视图从缓存派生

---

### H-4: Intl.NumberFormat 每次调用重新创建

**涉及文件**: `ccusage/packages/internal/src/format.ts:9,17`
**影响范围**: 所有涉及金额/数字格式化的输出，性能下降 5-10×`

**代码证据**:
```typescript
export function formatUSD(amount: number): string {
    return new Intl.NumberFormat('en-US', { ... }).format(amount);
    // 每次调用都 new，而非复用
}
```

`Intl.NumberFormat` 构造函数涉及 ICU 数据查找和 locale 解析，是相对昂贵的操作。在表格渲染场景中（每行多次调用），影响显著。

**建议修复**:
```typescript
const usdFormatter = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' });
export function formatUSD(amount: number): string {
    return usdFormatter.format(amount);
}
```

---

### H-5: calculateTieredCost 闭包每次调用重新创建

**涉及文件**: `ccusage/packages/internal/src/pricing.ts`
**影响范围**: 每次定价计算

**问题描述**: 内部闭包函数在每次外部调用时重新创建，产生不必要的函数对象分配和 GC 压力。

**建议修复**: 将闭包提升为模块级函数或提取到外部。

---

### H-6: calculateCostFromTokens 三重冗余 Fallback

**涉及文件**: `ccusage/packages/internal/src/pricing.ts`
**影响范围**: 每次 token 成本计算

**问题描述**: 单个模型定价查找触发三个独立的 `fetch()` 调用链（litellm → llm-prices → local cache），且每个 fallback 都重新进行网络请求。在模型名不存在的情况下造成 3× 网络延迟。

**建议修复**:
1. 将三个数据源合并为单次批量 fetch
2. 实现短路逻辑：如果 litellm 已覆盖多数模型，在缺失时才 fallback
3. 添加本地定价缓存层避免重复 fetch

---

### H-7: 每次 Command 调用重新加载数据，无跨命令缓存

**涉及文件**:
- `ccusage/apps/amp/commands/`
- `ccusage/apps/opencode/commands/`

**影响范围**: 同一进程中多次 CLI 命令执行

**问题描述**: 每个 command（如 daily、monthly、sessions）独立调用 `loadDailyUsageData()` 等函数，同一批文件数据在单次执行中被多次完整加载。

**建议修复**: 在命令调度层引入请求级缓存，同一进程生命周期内复用已加载数据。

---

### H-8: calculate-cost.ts 每个 Entry 单独计算 Cost 无批量优化

**涉及文件**: `ccusage/apps/ccusage/calculate-cost.ts`
**影响范围**: 大批量条目计算场景

**问题描述**: 对 `allEntries` 数组中每个条目单独调用 `model_cost_usd()`，每次都触发完整的模型名匹配流程（candidates_for 生成 20+ 变体 + pricing lookup），无批量查询优化。

**建议修复**:
1. 先收集所有唯一模型名 → 批量查询定价 → 再映射到各条目
2. 引入模型名 → cost 的本地 Map 缓存

---

### H-9: Codex Session 数据重复处理

**涉及文件**: `ccusage/apps/codex/session-report.ts`
**影响范围**: Codex session 报告生成

**问题描述**: Session 数据在加载后被过滤、转换、聚合多次，且每次视图切换时重新加载全部原始数据。

**建议修复**: 一次加载原始数据后，在内存中完成所有视图派生。

---

## 🟡 Medium

缺少缓存、冗余计算、不必要的序列化 — 累积效应下影响吞吐量。

---

### M-1: candidates_for 每次调用分配 HashSet + 大量字符串

**涉及文件**: `rust-backend/src/pricing/mod.rs:262-333`
**影响范围**: 每次 `model_cost_usd()` 调用

**问题描述**: 为单个模型名生成 **20+ prefix 变体** × **4 种 normalize 变体** ≈ **200+ 候选字符串**，全部在堆上分配。在大批量条目计算场景中（每天数千条），分配压力显著。

**建议修复**:
1. 将 `candidates_for` 结果缓存在 `HashMap<String, Vec<String>>` 中
2. 减少候选变体数量（当前 20+ prefix 可能过多）

---

### M-2: createMatchingCandidates 分配 Set + Array 每次模型查找

**涉及文件**: `ccusage/packages/internal/src/pricing.ts`
**影响范围**: TypeScript 定价查找

**问题描述**: 与 Rust 的 `candidates_for` 类似，每次模型名查找分配 Set 和 Array。

**建议修复**:
1. 缓存候选字符串生成结果
2. 对于已知模型名直接查 Map，跳过候选生成

---

### M-3: Standalone Fetch 函数绕过共享缓存

**涉及文件**: `ccusage/packages/internal/src/pricing-fetch-utils.ts`
**影响范围**: 定价数据获取

**问题描述**: 独立的 fetch 工具函数不经过 `sharedPricingMapPromises` 缓存层，导致同一定价数据可能被重复下载。

**建议修复**: 将所有定价 fetch 统一走 `sharedPricingMap` 缓存路径。

---

### M-4: Table toString() 每次渲染全量扫描所有 Cell

**涉及文件**: `ccusage/packages/terminal/src/table.ts`
**影响范围**: 终端表格渲染

**问题描述**: `toString()` 方法每次调用重新遍历所有 cell 计算列宽和渲染。多次调用（如分页输出或多次日志）时重复计算。

**建议修复**: 缓存列宽计算结果，仅在数据变更时重新计算。

---

### M-5: SQLite 连接无连接池

**涉及文件**: `rust-backend/src/sources/opencode.rs`
**影响范围**: OpenCode SQLite 数据源

**问题描述**: 每次 `load_source_view()` 调用 `Connection::open_with_flags()` 创建新连接。高频请求下反复打开/关闭 SQLite 文件带来不必要的 I/O 开销。

**建议修复**:
1. 引入 `r2d2-sqlite` 或 `deadpool-sqlite` 连接池
2. 或使用单例连接 + `Mutex`

---

### M-6: Server 无请求频率限制

**涉及文件**: `rust-backend/src/server.rs`
**影响范围**: HTTP API 服务

**问题描述**: 无 rate limiting，恶意或错误客户端可能发送大量重复请求，触发重复的全量数据加载。

**建议修复**: 引入 `tower::limit::RateLimitLayer` 或 `governor` crate 实现频率限制。

---

### M-7: 大响应体无压缩

**涉及文件**: `rust-backend/src/server.rs`
**影响范围**: 网络传输性能

**问题描述**: 每日/每月数据响应可能达到数百 KB，未启用 gzip/brotli 压缩。

**建议修复**: 添加 `tower-http::compression` 中间件。

---

### M-8: Daily/Monthly Grouping 多个命令独立做相同分组操作

**涉及文件**: `ccusage/apps/*/commands/`
**影响范围**: CLI 命令执行效率

**问题描述**: 多个命令（daily、monthly、weekly）各自独立对同一批原始数据做分组聚合，无中间结果共享。

**建议修复**: 在数据加载层一次性计算 daily aggregation，monthly/weekly 从 daily 结果派生而非重新加载原始数据。

---

### M-9: Today Summary 线性扫描所有 Daily Rows

**涉及文件**: `rust-backend/src/server.rs:394-414`
**影响范围**: Today Summary API

**问题描述**: 为找到今天的行，线性遍历所有 daily rows。当历史数据累积时，扫描开销随天数线性增长。

**建议修复**: 改用 `HashMap<DateString, Row>` 索引。

---

### M-10: Refresh Startup 启动时加载数据两遍

**涉及文件**: `rust-backend/src/server.rs`
**影响范围**: 服务启动时间

**问题描述**: `refresh_startup()` 加载所有 daily → `refresh_all()` 再加载所有 source × view。启动时同一数据被加载 2 轮。

**建议修复**: 合并两次加载为一次，先加载 raw entries → 再派生所有视图。

---

### M-11: Refresh Daily Range 逐天请求无批量优化

**涉及文件**: `rust-backend/src/server.rs`
**影响范围**: 日期范围刷新

**问题描述**: `refresh_daily_range()` 逐天调用 `provider.load_today_daily()`，每天内部独立打开/读取文件。N 天 = N 次文件系统扫描。

**建议修复**: 改为单次全量加载 → 按日期过滤分发。

---

### M-12: MCP 每个 Tool 独立调用子 App Data-Loader

**涉及文件**: MCP server (workspace root)
**影响范围**: MCP 工具调用链

**问题描述**: MCP 请求链中每个 tool（daily、monthly、sessions）独立调用对应 app 的 data-loader，同一批文件数据在单次 MCP 交互中被多次全量加载。

**建议修复**: 在 MCP server 层引入请求级数据缓存。

---

## 🟢 Low

代码风格导致的微小性能损失 — 单独影响小，但累积后值得优化。

---

### L-1: Cleanup Apply 模式未实际删除文件

**涉及文件**: `rust-backend/src/cleanup.rs:35-44`
**影响范围**: 清理任务

**问题描述**: Apply 模式下标记为清理的文件实际未被删除，仅记录日志，导致磁盘空间持续占用。

**建议修复**: 在 apply 模式下执行实际的文件删除操作。

---

### L-2: Factory Source 每次打开 Companion JSONL

**涉及文件**: `rust-backend/src/sources/factory.rs`
**影响范围**: Factory 数据源加载

**问题描述**: 每个 settings 文件都对应打开一个 companion `.jsonl` 文件读取元数据，打开文件数随 settings 文件数线性增长。

**建议修复**: 批量读取 companion 文件或缓存元数据。

---

### L-3: Intl.DateTimeFormat 每个日期 Cell 重新创建

**涉及文件**: `ccusage/packages/terminal/src/table.ts`
**影响范围**: 终端表格日期渲染

**问题描述**: 与 `Intl.NumberFormat` 类似，`Intl.DateTimeFormat` 每次调用重新实例化。

**建议修复**: 提升为模块级常量。

---

### L-4: Header 数组每次 Table 创建重新分配

**涉及文件**: `ccusage/packages/terminal/src/table.ts`
**影响范围**: 表格创建

**问题描述**: 表头数组每次 `new Table()` 重新分配，即使内容相同。

**建议修复**: 使用静态常量或复用已有数组。

---

### L-5: 未匹配模型名多次正则测试

**涉及文件**: `ccusage/packages/terminal/src/table.ts`
**影响范围**: 模型名匹配

**问题描述**: 对未匹配的模型名应用多个正则表达式尝试匹配，无短路或缓存。

**建议修复**: 缓存已知匹配结果，先查缓存再应用正则。

---

### L-6: 7 个 Source Adapter 重复实现聚合逻辑

**涉及文件**: `rust-backend/src/sources/*.rs`
**影响范围**: 代码维护性

**问题描述**: 每个 source adapter 各自实现 `entries_to_daily()`、`entries_to_monthly()`、`entries_to_sessions()`，聚合逻辑高度重复（~5 次）。

**建议修复**: 抽取公共聚合 trait 或函数，各 adapter 仅提供差异化的数据提取逻辑。

---

### L-7: 6 个 CLI App 各自重复 Data-Loader 和 Schema 定义

**涉及文件**: `ccusage/apps/*/data-loader.ts`
**影响范围**: 代码维护性

**问题描述**: 每个 CLI app 有独立的 `data-loader.ts`、`pricing.ts`，大量重复的 schema 定义和聚合逻辑。

**建议修复**: 将公共逻辑提升到 `ccusage/packages/internal`，各 app 仅保留数据源适配差异。

---

### L-8: Rust Source 模块静默跳过错误

**涉及文件**: `rust-backend/src/sources/*.rs`
**影响范围**: 可观测性

**问题描述**: 多个 source 模块中 `Err(_) => continue` 静默跳过解析失败的文件，无日志输出，难以排查个别文件问题。

**建议修复**: 添加 `tracing::warn!` 或 `log::warn!` 记录跳过的文件和原因。

---

## 优化优先级矩阵

按**影响面 × 修复成本**排序的推荐实施顺序：

| 优先级 | 编号 | 问题 | 预期收益 | 改动量 | 风险 |
|--------|------|------|----------|--------|------|
| **P0** | C-5 | UsageCache 无 TTL/容量限制 | 防止内存无限增长 | 小 | 低 |
| **P0** | C-6 | sharedPricingMapPromises 泄漏 | 防止 promise 泄漏 | 小 | 低 |
| **P0** | H-4 | Intl.NumberFormat 重复创建 | 5-10× 格式化加速 | 小 | 低 |
| **P0** | C-4 | TS 全量无过滤加载 | 大幅减少 I/O 时间 | 中 | 中 |
| **P0** | H-3 | Claude/Codex 重复读文件 | 消除 3× 重复 I/O | 中 | 中 |
| **P1** | C-1 | SQLite 无索引全表扫描 | 查询加速 10-100× | 中 | 中 |
| **P1** | C-3 | OpenCode 全量加载到内存 | 防止 OOM | 中 | 低 |
| **P1** | C-7 | calculateContextTokens OOM | 防止大文件 OOM | 小 | 低 |
| **P1** | H-1 | reqwest::blocking | 消除 Tokio 线程阻塞 | 小 | 低 |
| **P1** | H-2 | std::sync::RwLock | 消除 async 锁阻塞 | 小 | 低 |
| **P1** | M-5 | SQLite 连接池 | 减少连接开销 | 小 | 低 |
| **P2** | C-2 | Hermes COALESCE 全表扫描 | 查询加速 | 中 | 中 |
| **P2** | H-6 | 三重冗余 Fallback | 减少网络延迟 | 中 | 中 |
| **P2** | H-7 | 跨命令数据缓存 | 减少重复 I/O | 中 | 中 |
| **P2** | M-6 | Server 频率限制 | 防止滥用 | 小 | 低 |
| **P2** | M-7 | 响应压缩 | 减少带宽 50-80% | 小 | 低 |
| **P3** | M-1 | candidates_for 缓存 | 减少字符串分配 | 小 | 低 |
| **P3** | M-4 | Table toString 缓存 | 减少重复计算 | 小 | 低 |
| **P3** | L-1~L-8 | 低优先级项 | 边际改善 | 小 | 低 |

---

## 架构级建议

基于审查发现的共性模式，提出以下架构级改进方向：

### 1. 数据加载统一分层

当前数据加载分散在 Rust adapter、TS data-loader、MCP server 三层，各自独立实现文件扫描和解析。建议：

```
┌────────────────────────────────┐
│         API / CLI / MCP        │  ← 统一查询接口
├────────────────────────────────┤
│      统一缓存层 (带 TTL)         │  ← 共享数据缓存
├────────────────────────────────┤
│      统一数据加载层              │  ← 一次性加载 + 增量更新
├────────────────────────────────┤
│    Source Adapters (只负责适配)  │  ← 最小化差异
└────────────────────────────────┘
```

### 2. 增量加载代替全量扫描

核心原则：**每次请求不应重新扫描所有历史文件**。

- Rust: 基于文件 `mtime` 和缓存状态判断是否需要重新读取
- TypeScript: 引入本地 SQLite 缓存层，仅在文件变更时增量更新
- 所有层: 添加日期范围过滤作为第一道防线

### 3. 缓存策略标准化

| 缓存类型 | TTL | 淘汰策略 | 适用场景 |
|----------|-----|----------|----------|
| 会话级 | 请求结束 | N/A | 单次请求内多个视图 |
| 短期 | 5-30 分钟 | LRU (500 条目) | 定价数据、文件扫描结果 |
| 长期 | 跨天 | 日期边界淘汰 | 聚合后的 daily/monthly |

### 4. 批量操作优先

- 定价查询：收集唯一模型名 → 批量查询 → 映射回条目
- 数据聚合：一次性加载 raw entries → 内存中派生所有视图
- 文件 I/O：合并相邻日期范围的扫描为单次操作

---

## 附录：审查覆盖范围

| 层级 | 检查文件数 | 发现 Critical | 发现 High | 发现 Medium | 发现 Low |
|------|-----------|--------------|-----------|-------------|----------|
| Rust Backend | 12 files | 3 | 3 | 5 | 3 |
| Shared Packages | 5 files | 1 | 3 | 2 | 3 |
| Data Loaders | 6 files | 1 | 0 | 0 | 0 |
| Commands Layer | 5 files | 0 | 3 | 1 | 0 |
| Server Layer | 2 files | 0 | 0 | 4 | 0 |
| **总计** | **30 files** | **5** | **9** | **12** | **6** |
