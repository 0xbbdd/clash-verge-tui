# 上游版本与接口基线

本文说明 `clash-verge-tui` 所称“基于 Clash Verge”的确切含义。取证日期为 2026-08-13。

## 三个需要分开的版本

| 层次 | 基线 | 用途 |
|---|---|---|
| 本机 Clash Verge Rev | 2.5.2，tag commit `28f2efc504059b1dc75c793618b775c8e1b2a5f1` | 验证数据目录、profile schema、Unix Controller 和实际共存行为 |
| 本机 Mihomo | Meta v1.19.29，linux/amd64 | 验证所有 REST 读取、TUI 启动以及 Unix Socket 传输 |
| Clash Verge 源码审查 | `dev` commit `aadceba0642b8f58cf7c6e426ff29bf6846cbfe1`，应用版本 2.5.3 | 确认 GUI 哪些操作直接调用 Mihomo，哪些属于 Tauri 进程内 command |

本项目不是 Clash Verge Rev 的源码 fork。它没有链接 Clash Verge 的 Rust crates，也没有复制 React/Tauri 前端代码。GPL-3.0-only 许可证与上游保持一致，但实现是独立的终端客户端。

## 基线如何确认

本机安装包：

```text
package=clash-verge
version=2.5.2
architecture=amd64
status=install ok installed
```

上游 tag：

```text
28f2efc504059b1dc75c793618b775c8e1b2a5f1 refs/tags/v2.5.2
```

实际运行的内核：

```text
Mihomo Meta v1.19.29 linux amd64 with go1.26.5
```

源码审查固定到 commit，而不是浮动的 `dev` 分支。审查范围包括：

- `src/services/api.ts`：外部 IP 信息等前端网络访问；
- `src/services/cmds.ts`：前端调用的 Tauri command 清单；
- `src-tauri/src/cmd/clash.rs`：核心启动、停止、运行配置和 DNS command；
- `src-tauri/src/cmd/profile.rs`：订阅、Merge/Script enhance、profile 写入和 current profile；
- `src-tauri/src/core/service.rs`：平台 service 安装与提权路径。

## 接口对应关系

### 直接兼容：Mihomo Controller

以下能力不需要调用 Clash Verge 私有后端，TUI 直接使用 Mihomo 的版本化行为：

| 能力 | 请求 |
|---|---|
| 版本与运行配置 | `GET /version`、`GET/PATCH /configs` |
| 策略组与节点 | `GET /proxies`、`PUT /proxies/:name`、`GET /group/:name/delay` |
| 连接 | `GET/DELETE /connections`、`DELETE /connections/:id` |
| 规则 | `GET /rules`、`PATCH /rules/disable` |
| Provider | `GET/PUT /providers/{proxies,rules}`、healthcheck |
| 缓存与维护 | `/cache/*/flush`、`/upgrade`、`/upgrade/geo`、`/restart` |

本地 Clash Verge 2.5.2 使用 `external-controller-unix` 启动 Mihomo，因此 `api.rs` 在 Unix Socket 上建立 HTTP/1.1 连接；远程模式使用 HTTP(S) 和 Bearer secret。

### 本地兼容层

Mihomo 不负责以下 GUI 数据，因此 `local.rs` 只在发现本地 Clash Verge 数据目录时启用：

- 读取和原子更新 `profiles.yaml`；
- 校验、导入和更新 `profiles/*.yaml`；
- 汇总 app/service/sidecar 日志的尾部；
- Linux/GNOME 下通过 `gsettings` 切换系统代理，并同步 `verge.yaml` 状态。

本地写入不提供通用文件 API。文件名必须是单个路径组件；下载限制为 HTTP(S)、16 MiB 且必须能解析为包含 Mihomo 关键字段的 YAML；替换采用同目录 `0600` 临时文件、`fsync` 和原子 rename；删除先改名为 tombstone，索引更新失败时回滚。

### 不能由外部客户端等价替代

Clash Verge 2.5.2/2.5.3 的以下能力位于 Tauri 进程内 command，没有稳定、带鉴权的进程外 API：

- Merge/Script profile enhance 和可视化节点/规则编辑；
- WebDAV、本地备份、导入与恢复；
- service install/uninstall/repair 和 sidecar 切换；
- GUI 更新、托盘、主题、CSS 注入、全局热键和自启动；
- 诊断导出及平台专属工具。

TUI 会明确显示这条边界。例如切换 profile 只更新 `current`，不会声称已经运行 Merge/Script 或应用最终配置。完整覆盖这些能力需要在 Clash Verge 内增加 Unix-only、带鉴权、版本化 schema 的 companion IPC；开放任意文件读写 REST 不是可接受的替代方案。

## 兼容性承诺

- 已验证组合：Clash Verge Rev 2.5.2 + Mihomo v1.19.29 + Linux/GNOME。
- Mihomo HTTP Controller 模式按官方 REST API 工作，不要求 GUI 与 TUI 同机。
- 其他 Clash Verge 版本只要保持数据目录 schema 和 Mihomo Controller 行为，预计可兼容，但在实测前不声明已验证。
- macOS/Windows 可以使用远程/本地 HTTP Controller；系统代理本地适配目前只实现 Linux/GNOME。

## 上游链接

- [Clash Verge Rev v2.5.2](https://github.com/clash-verge-rev/clash-verge-rev/tree/v2.5.2)
- [Clash Verge Rev 审查提交](https://github.com/clash-verge-rev/clash-verge-rev/commit/aadceba0642b8f58cf7c6e426ff29bf6846cbfe1)
- [Mihomo REST API](https://wiki.metacubex.one/api/)

