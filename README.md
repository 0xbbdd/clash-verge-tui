# clash-verge-tui

面向 Clash Verge Rev / Mihomo 的终端控制台。它会优先自动发现 Clash Verge Rev 的 Unix Controller Socket，因此不需要为了 TUI 把控制端口暴露到 TCP；也可以连接远程 HTTP(S) Controller。

> 当前是可运行的 `0.1.0`：Mihomo 的日常控制面已完整接通，本地订阅和 Linux 系统代理也可管理。Clash Verge Rev 的一部分功能是进程内 Tauri command，并不存在可供外部程序调用的稳定 API；这部分不能诚实地声称已经等价实现，边界见下文。

## 上游版本基线

- 运行兼容基线：**Clash Verge Rev 2.5.2**，对应上游 tag [`v2.5.2`](https://github.com/clash-verge-rev/clash-verge-rev/tree/v2.5.2)，tag commit `28f2efc504059b1dc75c793618b775c8e1b2a5f1`。
- 内核联调基线：**Mihomo Meta v1.19.29**。
- 源码接口审查基线：Clash Verge Rev `dev` commit [`aadceba0642b`](https://github.com/clash-verge-rev/clash-verge-rev/commit/aadceba0642b8f58cf7c6e426ff29bf6846cbfe1)，该提交的应用版本为 2.5.3。

这里的“基于”是接口兼容而非源码 fork：项目没有嵌入 Clash Verge GUI 或复制其前端，而是通过 Mihomo REST/Unix Controller 完成运行时控制，并通过受限的本地适配层读取 Clash Verge 2.5.2 的 profile、日志和代理设置。版本取证、接口映射和兼容边界见 [上游基线说明](docs/UPSTREAM.md)，内部实现见 [架构说明](docs/ARCHITECTURE.md)。

## 安装与启动

要求 Rust 1.88+；本项目按 Rust 2024 edition 编写，锁定依赖已在 Rust 1.88.0 上构建和测试。

```bash
cd /home/bea1e/clash-verge-tui
cargo build --release
./target/release/clash-verge-tui
```

本机 Clash Verge Rev 正在运行时，通常无需参数。只检查自动发现、且不进入 TUI：

```bash
./target/release/clash-verge-tui --doctor
```

远程 Mihomo：

```bash
CVT_SECRET='controller-secret' ./target/release/clash-verge-tui \
  --controller http://127.0.0.1:9090
```

显式指定本地 socket：

```bash
./target/release/clash-verge-tui --socket /tmp/verge/verge-mihomo.sock
```

敏感信息只从 `--secret`、`CVT_SECRET` 或 Clash Verge 本地 YAML 读取；`--doctor` 永不打印 secret。

## 功能

| 页面 | 已实现操作 |
|---|---|
| Home | 核心状态/版本、会话流量、模式切换、TUN、Linux/GNOME 系统代理、LAN/IPv6 状态 |
| Proxies | 策略组、当前节点、节点选择、组延迟测试 |
| Connections | 实时连接、进程/目标/链路/流量、关闭单连接或全部连接 |
| Rules | 规则、命中数、运行时临时启用/禁用 |
| Providers | 代理/规则 Provider 查看、更新、代理 Provider 健康检查 |
| Profiles | Clash Verge 本地 profile 列表、订阅导入/更新/删除、标记 current；下载内容先做 YAML/大小校验并原子替换 |
| Logs | 汇总 Clash Verge app/service/sidecar 的本地日志并搜索 |
| Settings | allow-lan、IPv6、DNS/fake-IP 缓存、GEO 更新、Mihomo 自更新/重启 |

所有会中断网络、删除数据或升级组件的批量操作都有确认对话框。单连接关闭、单规则切换和节点选择以明确按键作为授权。

## 快捷键

- `1`–`8` 或 `Tab`：切换页面
- `j`/`k`、方向键：移动
- `g`/`G`：首项/末项
- `/`：搜索当前页面
- `r`：立即刷新
- `?`：帮助
- `q` / `Ctrl-C`：退出

每个页面的专用键会显示在底栏和边框标题中。

## 架构

```text
                         ┌─ HTTP over Unix socket ── Clash Verge 的 Mihomo
Terminal ── ratatui ── API client
                         └─ HTTP(S) + Bearer ─────── 远程 Mihomo
             │
             └─ LocalStore ── profiles.yaml / profiles/* / logs / gsettings
                                │
                                └─ 校验 → 0600 临时文件 → fsync → 原子 rename
```

数据面以官方 Mihomo REST API 为准；本地层只处理 Mihomo API 不拥有的 Clash Verge 文件和桌面代理。profile 文件名必须是单个安全路径组件，订阅限制为 HTTP(S)、16 MiB 以内且 YAML 根节点包含 Mihomo profile 关键字段。

源代码按职责拆分为控制器传输、数据解析、Clash Verge 本地适配、状态机和纯渲染层；详细的模块表、请求流、写入事务和信任边界见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 与 Clash Verge GUI 的覆盖边界

Clash Verge Rev GUI 同时使用两类接口：

1. Mihomo Controller API：代理、连接、规则、Provider、运行配置、缓存和核心维护。本 TUI 已直接覆盖。
2. Tauri 进程内 command：profile enhance（Merge/Script）、WebDAV/本地备份、服务安装修复、系统自启动、托盘/主题/热键、CSS 注入、诊断导出、图形编辑器等。它们没有受支持的进程外 IPC，Mihomo API 也不能代替。

因此当前版本的几个重要限制是：

- `Enter` 只原子更新 `profiles.yaml` 的 `current`，不会假装执行 Clash Verge 的 Merge/Script enhance；需要在 Clash Verge 中 reload/restart 后才会生成并应用最终运行配置。
- TUI 可以原子下载/更新原始订阅，但不会执行 JavaScript profile helper。
- WebDAV、备份恢复、service install/uninstall/repair、GUI 自更新、托盘/主题/热键等仍由 GUI 管理。
- 系统代理写操作当前实现 Linux/GNOME `gsettings`；macOS/Windows 后端尚未接入。
- 日志页在本地读取 Clash Verge 日志；远程 Controller 的 `/logs` 流式订阅尚未接入。

要达到字面意义上的“GUI 所有功能”，下一阶段应在 Clash Verge Rev 内加入一个带鉴权、Unix-only、版本化 schema 的 companion IPC bridge，复用其现有 Tauri command 后端；直接复制一个不受保护的文件 REST API 会扩大攻击面。本项目没有用不安全的文件服务伪造这层兼容性。

## 开发验证

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

接口依据：

- [Mihomo REST API](https://wiki.metacubex.one/api/)
- [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev)

License: GPL-3.0-only.
