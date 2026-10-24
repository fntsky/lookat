# Lookat 数据接收规范 v1

状态：**v1 已实现**。Chamber 仅接收经过 Formicary API 密钥鉴权的批次，持久化区间并提供网页查询；不采集服务器本机的前台应用。

## 1. 架构与鉴权边界

```text
用户电脑的采集端 → Formicary 密钥鉴权 → Lookat 校验 / 幂等入库 → 网页
```

上报通过 `Authorization: Bearer <Formicary API Key>` 或 `X-API-Key` 携带密钥。Formicary 验证并剥离密钥，转发 `X-Formicary-Auth: api_key`、`X-Formicary-User`、`X-Formicary-Principal: api-key:<key-id>` 和 `X-Formicary-Key-Id`。Chamber 检查完整身份上下文，缺少有效身份返回 401；仅浏览器 session 上报返回 403。读取接口接受 Formicary session 或 api_key。

不自行签发、缓存或验证密钥，不接受正文中的 user_id 或 token。Formicary 会剥离客户端伪造的 `X-Formicary-*`。**内部 upstream 仅监听 127.0.0.1，必须仅允许可信本机代理访问**；不应将其内部端口映射到公网，因为绕过代理的本机调用者可伪造身份头。外部请求必须经 Formicary。

当前 Formicary 密钥没有细分作用域或设备绑定。Lookat 以已验证的用户名隔离数据，首次合法上报即登记该用户下的设备；同一用户的其他有效密钥可继续上报该设备，轮换密钥不会重复生成统计。同名设备 UUID 在不同用户名下互不共享。设备 UUID 不是授权凭据。当前上游只提供用户名，账户改名后历史记录仍保留在原用户名下，需显式迁移，不会自动转移。

采集端负责名单匹配、前台计时和离线队列；Chamber 不修改客户端名单、不收集窗口标题。浏览器从受保护 API 读取数据，不把页面停留当作使用时间。内置示例模式不写数据库。

## 2. 传输约定

外部上报地址：`POST /lookat/api/v1/batches`，代理剥离挂载前缀后为 `/api/v1/batches`。

- 正式远程传输必须用 HTTPS；JSON UTF-8，`Content-Type: application/json`，不使用压缩请求体。
- 正文最多 **16 KiB（16384 字节）**，每批最多 **20** 个已结束区间；两项限制同时满足，超限由客户端拆批。该上限兼容当前 Formicary 代理。
- 字段及 JSON 类型见 `ingestion-v1.schema.json`；未知字段拒绝，破坏性字段变更提升 `schema_version`。
- 所有时间戳固定为 UTC RFC3339 毫秒精度（`2026-10-25T09:06:24.000Z`）；日期统计时再换算时区。时长都是整数毫秒，数值不得超过 JavaScript 安全整数上限。
- 建议前台变化时结束区间；持续停留时每 15 秒切出一个结束区间并上报，每段最长 60 秒。无使用区间时仍每 15 秒发送空区间列表和状态快照作为心跳。此频率是建议，接入时限流参数与 Formicary 协商。

## 3. 字段说明

| 字段 | 语义 |
| --- | --- |
| `schema_version` | 固定 `1` |
| `batch_id` | 本批 UUID；重试保持不变，包括整个正文与 sent_at |
| `stream_id` | 采集进程启动时生成的 UUID；重启生成新的，持久待传批次仍保留旧 stream_id |
| `sequence` | 同一 stream 内从 0 开始递增；每个新批次加一，重试不变 |
| `device.id` | 安装级稳定 UUID；重装/重置后变更，首次有效密钥上报时登记在已验证用户名下 |
| `device.name` | 展示名称，1–80 字符，不参与身份验证 |
| `device.platform` | `linux` / `windows` / `macos`；协议允许该值不代表当前采集 CLI 已支持所有平台 |
| `device.timezone` | 有效 IANA 时区，如 `Asia/Shanghai`；不接受固定偏移替代，接收端需验证其存在 |
| `agent` | 名称固定 `lookat`，版本 `major.minor.patch`，允许预发行后缀 |
| `sent_at` | 首次封装批次的时间；离线补传不改写 |
| `intervals` | 已结束有效前台区间，允许空数组；同一设备内不能发生重叠 |
| `snapshot` | 最新观测状态，仅用于“最近快照”，**绝不参与累计求和** |

`application` 包含 `id`（稳定应用 id，1–128 字符）、`name`（1–80 字符）、`process_name`（1–260 字符）；可选 `executable`（完整可执行路径，最多 4096 字符，建议默认省略）。应用 id 由采集端稳定映射，不使用 PID 作为 id；例如 `com.visualstudio.code`。v1 按 `(身份, device.id, application.id)` 分组，不自动把不同设备上的同名应用合并。

`interval` 包含稳定 UUID `id`、`application`、`started_at`、`ended_at`、`duration_ms`。区间使用半开区间 `[started_at, ended_at)`，相邻区间可以共用边界；`duration_ms` 范围 1–60000。仅记录名单中应用的有效前台时间，后台运行、暂停、锁屏和采集失败都不计入。空闲检测若采集端支持，应在进入 idle 时结束有效区间；不支持时不能伪称已过滤键鼠空闲。

`snapshot` 有两类：

- `state: active`：需要 `application`、`observed_at`、`since`、`session_ms`，可选 `pid`。表示名单中应用在前台的一次观测，session_ms 是本次连续有效使用时长，不是新增累计量。
- `state: idle | locked | paused | no_window`：只有 `state` 和 `observed_at`，不能夹带前一个应用或时长。`no_window` 同时表示无有效窗口、未匹配统计名单或无法获得可靠前台状态。

名称和可执行路径拒绝空白值及控制字符。本协议不上传窗口标题、网页 URL、文档名、命令行参数或按键内容。可执行路径属于可选信息；接收方不能把该路径用于打开服务器文件或执行命令。

## 4. 结构以外的语义校验

JSON Schema 只验证结构，接收端同时执行以下语义校验：

1. `started_at < ended_at <= sent_at`；`since <= observed_at <= sent_at`。`duration_ms` 来自单调时钟，须与 UTC 区间长度差值不超过 1000 ms；检测到系统时钟跳变时采集端结束/丢弃不可靠区间并重置基线。active 的 `session_ms` 也不得大于 since 到 observed_at 的墙钟跨度加 1000 ms。
2. 同批区间按 started_at 升序排列，UUID 唯一且互不重叠。与已入库区间也不能重叠；相同 interval id 的相同业务字段除外。
3. sent_at 最多领先服务器 received_at 120 秒；最早区间开始时间及快照观测时间最多落后 received_at 7 天。允许离线补传，超过窗口须明确报错，不能静默改成“今天”。当前未自动清理历史记录。
4. `(身份, device.id, stream_id, sequence)` 不允许映射到不同批次；`device.timezone` 必须存在于 IANA 时区库，不能用 timezone 字段绕过设备归属检查。
5. 同一设备多个采集进程不得同时计时；重启后新 stream 可以从 sequence 0 开始，但已入库有效区间仍不能重叠。

## 5. 幂等、重传与原子性

- 整批先完成鉴权、校验和冲突检查，再原子提交；一个区间无效则整批拒绝，不能半批入库。
- 批次幂等键为 `(已验证身份, device.id, batch_id)`。同键同业务字段返回 duplicate（即便原批次已超出重传窗口）；同键不同正文返回 409，不覆盖原数据。比较解码后的规范化字段值，JSON 空白与对象字段顺序不影响判定。
- 区间幂等键为 `(已验证身份, device.id, interval.id)`。跨批重传同一已存区间不再次累计；同键字段不同返回 409。
- 区间与快照分别处理：延迟/乱序批次中的有效历史区间可以入库；快照仅在 observed_at 更新且同 stream 的 sequence 不回退时替换。相同 observed_at 不覆盖；跨 stream 也以严格更新的 observed_at 为准。
- 建议客户端保留未确认批次，网络失败/408/429/5xx 时指数退避并加入抖动，最多等待 5 分钟；429 遵守 Retry-After。401/403 暂停发送并等待有效授权；其他 4xx 停止自动重试并报告原因。不重试未支持的接口版本。
- 只有收到整批成功响应后才删除本地待传批次。接收端必须先持久化再确认，幂等记录至少覆盖 7 天重传窗口。

## 6. 响应

成功：HTTP 200，以下 accepted 表示事务已持久化，不是内存排队成功。

```json
{
  "schema_version": 1,
  "batch_id": "8a88bf1a-bb41-44bf-8917-cc6acc266001",
  "status": "accepted",
  "accepted_intervals": 1,
  "duplicate_intervals": 0,
  "received_at": "2026-10-25T09:06:24.100Z"
}
```

整批重复时 status 为 `duplicate`，accepted_intervals 为 0。新批次允许部分区间曾在其他批次成功提交：跳过相同区间，计入 duplicate_intervals；两项计数之和必须等于提交区间数。

| HTTP 状态 | 含义 |
| --- | --- |
| 401 / 403 | Formicary 未认证 / session 不具备上报资格 |
| 409 | 批次、sequence、区间字段冲突或时间重叠 |
| 413 / 415 | 正文超限 / 非 JSON 或使用压缩正文 |
| 422 | schema、时间范围、时区或版本不合法 |
| 429 | 部署层限流可返回 Retry-After；Lookat 未实现独立限流 |
| 5xx | 临时失败，可按规则重试 |

错误体形状为 `{"error":{"code":"invalid_payload","message":"…","field":"intervals[0].ended_at"}}`，field 可为空或省略。正文格式错误使用 400。网关自身的错误格式由 Formicary 最终协议决定，客户端也要兼容非 JSON 的网关失败响应。

## 7. 汇总与“当前使用”展示

累计只累加去重后的 interval.duration_ms。按选择的报表时区将 UTC 区间在本地午夜边界拆分，按 UTC 边界跨度分摊整数毫秒，余数归最后一个分段，分段之和必须等于原时长。日历/夏令时由 IANA 时区处理，不能假设每天固定 24 小时；汇总不跨设备自动合并。

重复批次不刷新设备在线状态。设备超过 45 秒没有 received_at 更新标记为离线；最后快照 observed_at 距服务器时间超过 30 秒、领先超过 5 秒或设备已离线时标记为陈旧，显示“最后观测”，不能继续增长使用时间。**有效历史补传不能冒充当前使用**。网络在线状态与有效 active 状态分别展示；idle / locked / paused / no_window 清除当前使用标识。网页始终以服务器持久化区间为累计来源，不能把 snapshot.session_ms 再加到累计。

## 8. 查询接口

通过 Formicary 登录会话或密钥访问，所有结果按该请求的已验证用户名过滤：

- `GET /lookat/api/v1/devices` → `{"devices":[{"id":"…","name":"…","platform":"linux","timezone":"Asia/Shanghai","last_received_at":"…","online":true}]}`。
- `GET /lookat/api/v1/overview?device_id=<UUID>&period=today|week`。可附 `date=YYYY-MM-DD` 与有效 IANA `timezone`；默认使用设备时区的今天，week 是截至选定日期的连续 7 天。返回 date、period、timezone、total_ms（选定范围）、today_ms（当前本地日）、apps、current、observed_at、stale、state。
- 每个 apps 项包含 id、name、process、usage_ms（选定范围）、total_ms（全历史）、last（报表时区 HH:MM）与 current。历史应用保留在名单中，选定范围无使用时 usage_ms 为 0。
- current 为最近 active 快照的 `{id,name,process,session_ms}` 或 null。stale=true 时仅作最后观测展示，不表示当前仍在使用；网页不在两次上报之间擅自递增时长。
- 未授权的设备 UUID 返回 404，不透露其他用户是否拥有该设备；未知 API 返回 404。

`GET /lookat/api/status` 是不含用户信息的接收端状态，`GET /lookat/healthz` 为健康检查。静态页面、协议文件和内置示例可公开访问，真实设备与统计数据必须经过鉴权。API 响应设置 Cache-Control: no-store。

## 9. 存储和实现范围

SQLite 默认为 `data/lookat.sqlite3`，独立于 Formicary 鉴权库。按版本迁移；批次、区间和设备快照在同一事务写入，正常重启后恢复，未引入自动清理。设备/批次/区间均按已验证用户名隔离，区间幂等记录随原记录保留。

已实现：Formicary 代理身份接入、结构与时间语义校验、幂等事务、重叠冲突回滚、UTC 区间汇总、跨天和夏令时处理、快照陈旧判定、受保护的设备/使用查询、移动端网页。

尚未实现：采集端自动上报及离线重试队列、客户端名单远程配置、数据删除与保留策略、独立设备授权。当前 `cargo run -- --watch` 仅本机输出，不会自动上传；可按上述规范由调用方提交批次。协议中的重试规则是采集端应遵循的约定。
