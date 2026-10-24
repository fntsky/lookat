# Lookat · Formicary Chamber

Rust + Vue 的应用使用记录接收端。纸色与橄榄绿搭配粗边框、亮色卡片和硬阴影；首页显示“我在干什么”、当前应用与使用时间，支持手机布局，不做应用类别分析。

## 数据流

```text
用户电脑的采集端 → Formicary 密钥鉴权 → Lookat SQLite → 网页
```

Chamber 不采集服务器的前台应用。已接入 Formicary API Key：上报仅接受 api_key 身份，网页查询接受登录会话或密钥。密钥验证与撤销由 Formicary 完成，Lookat 不签发或缓存密钥。真实数据默认展示；“查看示例”切换到明确标识的静态设计样例，不写数据库。

前台应用卡片显示最新快照，本次时长不与区间重复相加；统计按设备时区划分今日或近 7 天。设备离线或快照陈旧时显示最后观测，不在网页上继续增加时长。

## 构建与挂载

需要 Rust、Node.js 与 npm；接收端不需要图形桌面、KWin 或 D-Bus。

```sh
make build
```

产物为 `dist/lookat-server` 与 `frontend/dist/`。将项目与产物放入 Formicary Chamber 根目录的 `lookat/` 中，保留可执行的 start.sh。`chamber.toml` 通过 proxyhttp 把 `/lookat` 转发到 `127.0.0.1:53214`，重启 Formicary 后访问：

```text
http://127.0.0.1:53212/lookat/
```

网页未登录时可前往 `/ui` 使用 Formicary 登录。密钥在 Formicary 的“API 密钥”页面创建。内部 upstream 必须只允许可信本机代理访问，不应暴露到公网；客户端伪造身份头会由 Formicary 剥离。

| 变量 | 默认值 |
| --- | --- |
| LOOKAT_PORT | 53214（变更时同步 chamber.toml 的 upstream） |
| LOOKAT_FRONTEND_DIR | frontend/dist，start.sh 转为项目内绝对路径 |
| LOOKAT_DATA_PATH | data/lookat.sqlite3，start.sh 转为项目内绝对路径 |

统计库与 Formicary 鉴权库分离，迁移位于 `src/migrations/`，不会读取旧实验的 `data/usage.json`。数据按上游验证后的用户名隔离；当前上游不提供稳定用户 ID，改名后历史仍属于旧用户名，需要显式迁移。

## 上报与查询

- `POST /lookat/api/v1/batches`：Formicary API 密钥必需，session 不可用于上报。
- `GET /lookat/api/v1/devices`：当前用户的设备。
- `GET /lookat/api/v1/overview?device_id=<UUID>&period=today|week`：当前用户的设备使用情况。
- `GET /lookat/api/status`：不含用户信息的接收状态。
- `GET /lookat/healthz`：健康检查。

[完整接收规范](docs/ingestion-v1.md) · [JSON Schema](docs/ingestion-v1.schema.json) · [使用中示例](docs/examples/active-batch.json) · [锁屏心跳](docs/examples/locked-heartbeat.json)

```sh
curl -H "Authorization: Bearer $FORMICARY_API_KEY" \
  -H 'Content-Type: application/json' \
  --data-binary @batch.json \
  https://your-formicary.example/lookat/api/v1/batches
```

batch.json 需使用实际设备 UUID、时间戳与已结束的有效前台区间；文档示例时间是固定演示值，超过 7 天窗口需替换。正文最多 16 KiB、20 区间，单段最长 60 秒。通过批次和区间标识去重；冲突整批回滚，确认前已持久化。

## 开发与检查

```sh
npm ci --prefix frontend
make check
```

覆盖 Rust 格式与 Clippy、校验/鉴权/事务/用户隔离/持久化/跨天测试、Schema 示例测试、Vue 构建和真实接收端 HTTP 检查。

真实 Formicary 集成测试（需兄弟目录存在已构建的 Formicary release 二进制）：

```sh
python3 tests/formicary_smoke.py
```

测试在 Formicary 的 tmp 下创建独立鉴权库与 Chamber，使用临时管理员签发和撤销测试密钥，验证真实代理链路，不修改既有账号或数据库；退出会清理该测试目录与进程。`--browser` 可保留该隔离环境用于人工 UI 验证，输入换行退出。

前端开发：`npm run dev --prefix frontend`，默认 `http://127.0.0.1:5174/lookat/`。真实数据由开发代理转发给本机 Formicary 的 53212 端口，需要该地址的有效登录 Cookie；也可以切换示例模式。构建 base 为 `/lookat/`，更改挂载路径需同步 `frontend/vite.config.js` 并重建。

`make copy-to-chamber` 将源码与产物复制到 `../formicary/tmp/chamber/lookat`，用于本地开发运行，保留该副本的 data/，不会启动 Formicary。

## 本机 CLI

```sh
cargo run
cargo run -- --watch
```

原 CLI 独立输出本机前台窗口信息，支持 Windows、Linux X11（xprop）和 KDE Plasma 6 Wayland（直接调用 KWin）。它不会自动上报数据。采集端发送/离线队列、远程名单配置和数据清理策略不在本次接收端实现范围内。
