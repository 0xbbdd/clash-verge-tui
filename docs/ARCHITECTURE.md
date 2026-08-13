# 架构与代码审查说明

## 总体数据流

```text
keyboard / timer
      │
      ▼
App state machine ───────────────► pure Ratatui renderer
      │                                │
      │ mutations                     └─ full-frame buffer
      ▼                                      │
MihomoClient                                ▼
  ├─ HTTP/1.1 over UnixStream       Crossterm alternate screen
  └─ HTTPS + Bearer via reqwest
      │
      ├─ /configs /proxies /connections /rules /providers
      └─ explicit confirmed maintenance operations

LocalStore (only when a local Clash Verge data directory exists)
  ├─ profiles.yaml + profiles/*.yaml
  ├─ bounded log tails
  └─ Linux/GNOME gsettings
```

每次刷新并行请求七个独立端点；任何单项失败都会保留为显式错误，而不是把静态文件或部分成功解释成完整运行证据。页面切换会请求一次 full redraw，主循环调用 `Terminal::clear()` 重置真实终端和 Ratatui previous buffer，避免差分刷写残留。

## 模块职责

| 文件 | 职责 | 主要不变量 |
|---|---|---|
| `src/config.rs` | CLI、环境变量、数据目录与 controller 自动发现 | `--socket` 优先；secret 不在 doctor 输出中出现 |
| `src/api.rs` | Mihomo 请求、Unix/TCP 传输、状态码和 JSON 校验 | 每个路径参数 URL encode；请求有连接/响应超时 |
| `src/model.rs` | 宽松解析 Mihomo JSON 为稳定 UI snapshot | 缺字段使用保守默认值；规则使用 API 提供的真实 index |
| `src/local.rs` | Clash Verge profile、日志和系统代理适配 | 受限路径、下载校验、原子写入、删除回滚、日志读取有界 |
| `src/app.rs` | Tab、选择、输入、确认框、刷新和 mutation 状态机 | 高影响批量操作必须确认；搜索不改变底层数据 |
| `src/ui.rs` | 纯 Ratatui 绘制 | 不执行 I/O 或 mutation；页面只消费 `App` snapshot |
| `src/main.rs` | terminal 生命周期与事件循环 | 首帧/页面切换完整重绘；正常错误和 panic 都恢复 terminal |

## 启动与发现顺序

1. 解析 CLI 和 `CVT_*` 环境变量。
2. 如果提供 `--socket`，使用该 Unix Socket。
3. 否则如果提供 `--controller`，使用显式 HTTP(S) Controller。
4. 否则查找 Clash Verge 数据目录中的 `clash-verge.yaml`、`config.yaml`：优先存在的 `external-controller-unix`，再使用 `external-controller`。
5. 最后尝试 Clash Verge 常用的 `/tmp/verge/verge-mihomo.sock`。
6. secret 优先级是 CLI/环境变量，然后是本地 YAML；`--doctor` 只打印是否配置，不打印值。

没有本地数据目录时仍可完整使用 Mihomo 控制页面，Profiles/本地日志/系统代理会清楚标记为不可用。

## 读取路径

`App::refresh` 通过 `tokio::join!` 并行读取：

```text
/version  /configs  /proxies  /connections
/rules    /providers/proxies  /providers/rules
```

结果进入一次性 `Snapshot`，避免 UI 在同一帧混用不同请求周期的数据。本地 profiles 和最多三个日志文件的尾部随后合并；单个日志最多读取末尾 2 MiB，最终 UI 保留 1500 行。

## 写入与确认

| 操作 | 防护 |
|---|---|
| 选择节点、关闭单连接、临时切换单规则 | 用户按下页面专用键即构成明确授权 |
| 关闭全部连接、缓存清理、升级、重启、系统代理 | 必须通过确认对话框 |
| 订阅下载 | 仅 HTTP(S)，30 秒超时、16 MiB 上限、YAML schema 最低检查 |
| profile 写入 | 同目录临时文件，Unix 权限 `0600`，写入后 `fsync`，原子替换 |
| profile 删除 | 禁止删除 current；先 rename 为 tombstone，索引失败则恢复 |

订阅 URL 属于敏感配置，不在列表页显示。Controller secret 可以来自环境变量，避免出现在命令历史；HTTP 模式使用 Bearer header，Unix 模式也保持相同认证语义。

## Terminal 生命周期

应用自行管理 raw mode 和 alternate screen：

- 初始化失败会立即恢复 raw mode/alternate screen；
- 正常退出恢复屏幕并显示光标；
- panic hook 在输出 panic 前先恢复终端；
- 首帧、Tab 切换以及策略组/节点子页面切换请求一次完整 clear，普通滚动仍使用差分绘制。

## 审查结果

发布前完整检查覆盖全部 Rust 源文件、manifest、锁文件和文档。修正了：

- 锁定依赖的真实 MSRV 是 Rust 1.88，而不是 1.85；
- 日志从无界整文件读取改为每文件最多 2 MiB 的尾部读取；
- profile 删除增加 tombstone 和索引失败回滚；
- profile UID 改为纳秒时间戳，避免同秒导入冲突；
- 本地 profile/log 错误不再被 API 成功状态覆盖；
- panic 路径恢复终端；
- Tab 页面切换显式同步终端缓冲。

验证命令：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo +1.88.0 check --locked
cargo +1.88.0 test --locked
```
