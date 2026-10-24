<script setup>
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import Icon from "./components/Icon.vue";
import { sessionView } from "./session-clock";
import { devices as demoDevices, formatDuration, sampleApps } from "./demo";
import samplePayload from "../../docs/examples/active-batch.json";

const view = ref("overview");
const mode = ref("live");
const period = ref("today");
const deviceId = ref("");
const search = ref("");
const sort = ref("duration");
const detailDialog = ref(null);
const selectedApp = ref(null);
const copied = ref(false);
const message = ref("");
const base = import.meta.env.BASE_URL;
const devices = ref([]);
const report = ref(null);
const clock = ref(performance.now());
const reportReceivedAt = ref(0);
const session = computed(() =>
  sessionView(report.value, reportReceivedAt.value, clock.value),
);
const stale = computed(() => !isDemo.value && session.value.stale);
const loading = ref(false);
const isDemo = computed(() => mode.value === "demo");
const sources = computed(() => (isDemo.value ? demoDevices : devices.value));
const device = computed(() =>
  sources.value.find((item) => item.id === deviceId.value),
);
const apps = computed(() =>
  isDemo.value ? sampleApps(deviceId.value) : report.value?.apps || [],
);
const current = computed(() =>
  isDemo.value ? apps.value.find((app) => app.current) : report.value?.current,
);
const valueOf = (app) =>
  isDemo.value
    ? period.value === "week"
      ? app.week
      : app.seconds
    : Math.floor(app.usage_ms / 1000);
const total = computed(() =>
  isDemo.value
    ? apps.value.reduce((sum, app) => sum + valueOf(app), 0)
    : Math.floor((report.value?.total_ms || 0) / 1000),
);
const rows = computed(() =>
  [...apps.value]
    .filter((app) =>
      `${app.name} ${app.process}`
        .toLowerCase()
        .includes(search.value.trim().toLowerCase()),
    )
    .sort((a, b) =>
      sort.value === "name"
        ? a.name.localeCompare(b.name)
        : valueOf(b) - valueOf(a),
    ),
);
const currentSession = computed(() =>
  isDemo.value
    ? current.value?.session
    : session.value.seconds,
);
const currentName = computed(() => current.value?.name || "暂无前台应用");
const observed = computed(() =>
  isDemo.value
    ? device.value?.observed
    : report.value?.observed_at
      ? new Intl.DateTimeFormat("zh-CN", {
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
          timeZone: report.value.timezone,
        }).format(new Date(report.value.observed_at))
      : "—",
);
const protocolExample = JSON.stringify(samplePayload, null, 2);
let generation = 0;
let timer;
let clockTimer;
let disposed = false;

async function request(path) {
  const response = await fetch(`${base}api/v1/${path}`, {
    credentials: "same-origin",
    cache: "no-store",
    signal: AbortSignal.timeout(8000),
  });
  const data = await response.json().catch(() => ({}));
  if (
    response.ok &&
    !response.headers.get("content-type")?.includes("application/json")
  )
    throw new Error("请通过 Formicary 打开");
  if (!response.ok)
    throw new Error(data.error?.message || `请求失败 (${response.status})`);
  return data;
}
async function refresh() {
  if (isDemo.value || disposed) return;
  const version = ++generation;
  loading.value = true;
  try {
    const data = await request("devices");
    if (version !== generation || disposed) return;
    if (!Array.isArray(data.devices)) throw new Error("无法读取设备列表");
    devices.value = data.devices;
    if (!devices.value.some((d) => d.id === deviceId.value))
      deviceId.value = devices.value[0]?.id || "";
    if (deviceId.value) {
      const result = await request(
        `overview?device_id=${encodeURIComponent(deviceId.value)}&period=${period.value}`,
      );
      if (version === generation && !disposed) {
        reportReceivedAt.value = performance.now();
        clock.value = reportReceivedAt.value;
        report.value = result;
      }
    } else report.value = null;
    if (version === generation) message.value = "";
  } catch (error) {
    if (version === generation && !disposed) {
      message.value =
        error.name === "TimeoutError"
          ? "连接超时"
          : error.name === "TypeError"
            ? "无法连接服务"
            : error.message;
      report.value = null;
    }
  } finally {
    if (version === generation) loading.value = false;
  }
}
function changeMode() {
  generation++;
  mode.value = isDemo.value ? "live" : "demo";
  deviceId.value = isDemo.value ? demoDevices[0].id : "";
  report.value = null;
  message.value = "";
  loading.value = false;
  if (!isDemo.value) refresh();
}
watch([deviceId, period], () => {
  search.value = "";
  report.value = null;
  if (!isDemo.value) refresh();
});
function showDetail(app) {
  selectedApp.value = app;
  detailDialog.value.showModal();
}
async function copyExample() {
  try {
    await navigator.clipboard.writeText(protocolExample);
    copied.value = true;
  } catch {
    message.value = "可直接下载 JSON 示例";
  }
}
function exportCsv() {
  const quote = (value) => {
    const text = String(value ?? "");
    return `"${(/^[=+\-@\t\r]/.test(text) ? "'" + text : text).replaceAll('"', '""')}"`;
  };
  const records = [
    ["数据性质", "设备", "应用", "进程", "时长(秒)"],
    ...rows.value.map((app) => [
      isDemo.value ? "示例" : "接收记录",
      device.value?.name,
      app.name,
      app.process,
      valueOf(app),
    ]),
  ];
  const url = URL.createObjectURL(
    new Blob(
      ["\ufeff" + records.map((row) => row.map(quote).join(",")).join("\r\n")],
      { type: "text/csv;charset=utf-8;" },
    ),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `lookat-${isDemo.value ? "demo-" : ""}${period.value}.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function onVisibilityChange() {
  if (!document.hidden) {
    clock.value = performance.now();
    refresh();
  }
}
onMounted(() => {
  refresh();
  clockTimer = setInterval(() => {
    clock.value = performance.now();
  }, 1000);
  document.addEventListener("visibilitychange", onVisibilityChange);
  timer = setInterval(() => {
    if (!document.hidden) refresh();
  }, 10000);
});
onUnmounted(() => {
  disposed = true;
  generation++;
  clearInterval(timer);
  clearInterval(clockTimer);
  document.removeEventListener("visibilitychange", onVisibilityChange);
});
</script>

<template>
  <div class="site-shell">
    <header class="site-header">
      <a
        class="brand"
        href="#"
        @click.prevent="view = 'overview'"
        aria-label="Lookat 首页"
        ><span class="brand-eye"><Icon name="eye" :size="26" /></span
        >LOOKAT<span class="brand-period">.</span></a
      >
      <nav aria-label="主导航">
        <button
          :class="{ active: view === 'overview' }"
          @click="view = 'overview'"
        >
          当前活动</button
        ><button
          :class="{ active: view === 'applications' }"
          @click="view = 'applications'"
        >
          使用记录</button
        ><button
          :class="{ active: view === 'protocol' }"
          @click="view = 'protocol'"
        >
          接收规范
        </button>
      </nav>
      <a class="workspace-link" href="/ui"
        >Formicary <Icon name="arrow" :size="15"
      /></a>
    </header>
    <div class="mode-strip">
      <span
        ><i :class="{ demo: isDemo }"></i
        >{{ isDemo ? "示例数据" : "接收端" }}</span
      ><button @click="changeMode">
        {{ isDemo ? "返回真实数据" : "查看示例" }} ↗
      </button>
    </div>
    <div v-if="message" class="notice" role="alert">
      <span>{{ message }}</span
      ><button v-if="!isDemo" @click="refresh">重试</button>
    </div>
    <section class="page-heading">
      <div>
        <p class="eyebrow">
          LOOKAT / {{ view === "protocol" ? "CONTRACT V1" : "ACTIVITY" }}
        </p>
        <h1>
          {{
            view === "overview"
              ? "我在干什么"
              : view === "applications"
                ? "使用记录"
                : "接收规范"
          }}<span>.</span>
        </h1>
      </div>
      <label v-if="view !== 'protocol'" class="device-select"
        ><Icon name="apps" :size="17" /><select
          v-model="deviceId"
          aria-label="选择设备"
          :disabled="!sources.length"
        >
          <option v-if="!sources.length" value="">暂无设备</option>
          <option v-for="item in sources" :value="item.id" :key="item.id">
            {{ item.name }}
          </option>
        </select></label
      ><span v-else class="small-chip">v1 / JSON</span>
    </section>

    <template v-if="view !== 'protocol'">
      <section v-if="view === 'overview'" class="now-grid">
        <article class="current-panel">
          <div class="panel-kicker">
            <span>{{ stale ? "最后应用" : "当前应用" }}</span
            ><span class="snapshot-tag">{{
              isDemo
                ? "示例快照"
                : stale
                  ? "最后观测"
                  : current
                    ? "实时估算"
                    : "等待上报"
            }}</span>
          </div>
          <div class="current-app">
            <span :class="['current-icon', current?.tone || 'lime']">{{
              current?.short || current?.name?.slice(0, 2).toUpperCase() || "—"
            }}</span>
            <div>
              <h2>{{ currentName }}</h2>
              <p>{{ current?.process || "—" }}</p>
            </div>
          </div>
          <div class="session-time">
            <span>本次使用</span
            ><strong>{{
              current ? formatDuration(currentSession, true) : "--:--:--"
            }}</strong>
          </div>
          <div class="current-meta">
            <span>{{ device?.name || "暂无设备" }}</span
            ><span>{{ observed }}</span>
          </div>
        </article>
        <aside class="totals-panel">
          <div>
            <span>今日使用</span
            ><strong>{{
              formatDuration(
                isDemo
                  ? apps.reduce((sum, app) => sum + app.seconds, 0)
                  : Math.floor((report?.today_ms || 0) / 1000),
              )
            }}</strong>
          </div>
          <div>
            <span>已记录应用</span
            ><strong
              >{{ String(apps.length).padStart(2, "0")
              }}<small> APPS</small></strong
            >
          </div>
          <div>
            <span>设备状态</span
            ><strong class="device-state"
              >{{
                isDemo
                  ? "示例"
                  : device?.online
                    ? "在线"
                    : device
                      ? "离线"
                      : "未连接"
              }}<i :class="{ online: !isDemo && device?.online }"></i
            ></strong>
          </div>
        </aside>
      </section>
      <section class="records-panel">
        <div class="records-heading">
          <h2>
            应用记录 <span>{{ String(apps.length).padStart(2, "0") }}</span>
          </h2>
          <div class="period-switch">
            <button
              :aria-pressed="period === 'today'"
              :class="{ selected: period === 'today' }"
              @click="period = 'today'"
            >
              今天</button
            ><button
              :aria-pressed="period === 'week'"
              :class="{ selected: period === 'week' }"
              @click="period = 'week'"
            >
              近 7 天
            </button>
          </div>
        </div>
        <div class="record-toolbar">
          <label class="search-control"
            ><Icon name="search" :size="17" /><input
              v-model="search"
              type="search"
              aria-label="搜索应用"
              placeholder="搜索应用" /></label
          ><span class="period-total"
            >{{ period === "week" ? "本周" : "今日" }}
            {{ formatDuration(total) }}</span
          ><button
            class="export-button"
            :disabled="!rows.length"
            @click="exportCsv"
          >
            导出 <Icon name="arrow" :size="15" />
          </button>
        </div>
        <div v-if="!apps.length" class="empty-state">
          <span>[ — ]</span>
          <h3>{{ loading ? "正在读取…" : "等待应用记录" }}</h3>
          <button class="text-button" @click="view = 'protocol'">接收规范 ↗</button>
        </div>
        <div v-else-if="!rows.length" class="empty-state">
          <h3>没有匹配的应用</h3>
          <button class="text-button" @click="search = ''">清空搜索</button>
        </div>
        <template v-else
          ><div class="list-head">
            <span>应用 / 进程</span
            ><button @click="sort = sort === 'duration' ? 'name' : 'duration'">
              使用时间 {{ sort === "duration" ? "↓" : "A–Z" }}</button
            ><span>最后观测</span><span></span>
          </div>
          <div class="app-list">
            <button
              v-for="app in rows"
              :key="app.id"
              class="app-row"
              :aria-label="`查看${app.name}详情`"
              @click="showDetail(app)"
            >
              <span class="app-identity"
                ><span :class="['app-avatar', app.tone || 'paper']">{{
                  app.short || app.name.slice(0, 2).toUpperCase()
                }}</span
                ><span
                  ><strong>{{ app.name }}</strong
                  ><span class="process-label">{{ app.process }}</span></span
                ></span
              ><span class="row-time">{{ formatDuration(valueOf(app)) }}</span
              ><span class="last-seen"
                ><span v-if="app.current" class="small-chip">{{
                  stale ? "最后观测" : "最近快照"
                }}</span
                ><template v-else>{{ app.last || "—" }}</template></span
              ><Icon name="arrow" class="row-arrow" :size="19" />
            </button></div
        ></template>
      </section>
    </template>

    <template v-else
      ><section class="protocol-layout">
        <article class="contract-panel">
          <div class="endpoint-line">
            <span>POST</span><code>/lookat/api/v1/batches</code>
          </div>
          <dl>
            <div>
              <dt>鉴权</dt>
              <dd>Formicary API Key</dd>
            </div>
            <div>
              <dt>正文</dt>
              <dd>JSON · ≤ 16 KiB</dd>
            </div>
            <div>
              <dt>区间</dt>
              <dd>≤ 20 条 / 批 · ≤ 60 秒 / 条</dd>
            </div>
            <div>
              <dt>时间</dt>
              <dd>UTC · 毫秒</dd>
            </div>
            <div>
              <dt>去重</dt>
              <dd>batch_id / interval.id</dd>
            </div>
            <div>
              <dt>快照</dt>
              <dd>不计入累计时长</dd>
            </div>
          </dl>
          <div class="download-links">
            <a :href="`${base}protocol/ingestion-v1.md`" download>完整规范 ↗</a
            ><a :href="`${base}protocol/ingestion-v1.schema.json`" download
              >JSON Schema ↗</a
            >
          </div>
        </article>
        <article class="code-panel">
          <div class="code-heading">
            <span>active-batch.json</span
            ><button @click="copyExample">
              {{ copied ? "已复制 ✓" : "复制" }}
            </button>
          </div>
          <pre
            tabindex="0"
            aria-label="上报 JSON 示例"
          ><code>{{ protocolExample }}</code></pre>
          <a :href="`${base}protocol/active-batch.json`" download
            >下载 JSON ↗</a
          >
        </article>
      </section></template
    >
    <footer class="site-footer">
      <span>LOOKAT / FORMICARY CHAMBER</span
      ><span>{{ isDemo ? "SAMPLE DATA" : "RECEIVER" }}</span>
    </footer>
  </div>
  <dialog
    ref="detailDialog"
    class="detail-dialog"
    aria-labelledby="detail-title"
  >
    <template v-if="selectedApp"
      ><div class="detail-top">
        <span class="small-chip">{{ isDemo ? "示例记录" : "应用记录" }}</span
        ><button aria-label="关闭应用详情" @click="detailDialog.close()">
          <Icon name="close" />
        </button>
      </div>
      <h2 id="detail-title">{{ selectedApp.name }}</h2>
      <p class="detail-id">{{ selectedApp.process }}</p>
      <div class="detail-total">
        <span>{{ period === "week" ? "本周" : "今日" }}使用</span
        ><strong>{{ formatDuration(valueOf(selectedApp), true) }}</strong>
      </div>
      <dl>
        <div>
          <dt>设备</dt>
          <dd>{{ device?.name }}</dd>
        </div>
        <div>
          <dt>应用 ID</dt>
          <dd>{{ selectedApp.id }}</dd>
        </div>
        <div>
          <dt>最后观测</dt>
          <dd>{{ selectedApp.last || "—" }}</dd>
        </div>
      </dl>
      <button class="brutal-button" @click="detailDialog.close()">
        关闭
      </button></template
    >
  </dialog>
</template>
