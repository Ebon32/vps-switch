<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import OsBadge from './OsBadge.vue';
import {
  globalInspectionCache as inspectionCache,
  ROLE_META,
  type DiskUsageItem,
  type HostInspection,
  type HostRole,
  type PublicHost,
} from '../types';

const netHistoryCache = new Map<string, Array<{ rx: number; tx: number }>>();

const props = defineProps<{
  host: PublicHost;
  checking: boolean;
}>();

const emit = defineEmits<{
  close: [];
  check: [host: PublicHost];
  edit: [host: PublicHost];
  delete: [host: PublicHost];
  openTerminal: [host: PublicHost];
  openFiles: [host: PublicHost];
  patchMeta: [host: PublicHost, patch: Record<string, unknown>];
  hostUpdated: [host: PublicHost];
}>();

const tagInput = ref('');
const snap = ref<HostInspection | null>(null);
const inspecting = ref(false);
const inspectErr = ref<string | null>(null);
const netBars = ref<Array<{ rx: number; tx: number }>>([]);

let inspectTimer: ReturnType<typeof setInterval> | null = null;
let reqSeq = 0;

const osPretty = computed(() => {
  return (
    props.host.osInfo?.prettyName ||
    props.host.osInfo?.distroId ||
    props.host.osInfo?.os ||
    '正在探测系统...'
  );
});

const cpuLabel = computed(() => {
  return (
    snap.value?.cpuModel ||
    props.host.osInfo?.cpuModel ||
    props.host.osInfo?.arch ||
    '正在探测处理器型号...'
  );
});

const memUsedPct = computed(() => {
  if (!snap.value || !snap.value.memTotalBytes) return 0;
  return Math.min(100, Math.round((snap.value.memUsedBytes * 100) / snap.value.memTotalBytes));
});

const memCachePct = computed(() => {
  if (!snap.value || !snap.value.memTotalBytes) return 0;
  const raw = Math.round((snap.value.memCachedBytes * 100) / snap.value.memTotalBytes);
  return Math.min(100 - memUsedPct.value, Math.max(0, raw));
});

const swapUsedPct = computed(() => {
  if (!snap.value || !snap.value.swapTotalBytes) return 0;
  return Math.min(100, Math.round((snap.value.swapUsedBytes * 100) / snap.value.swapTotalBytes));
});

const diskList = computed<DiskUsageItem[]>(() => {
  if (!snap.value) return [];
  if (snap.value.disks && snap.value.disks.length > 0) {
    return snap.value.disks;
  }
  if (snap.value.diskTotalBytes > 0) {
    return [
      {
        mount: snap.value.diskMount || '/',
        totalBytes: snap.value.diskTotalBytes,
        usedBytes: snap.value.diskUsedBytes,
      },
    ];
  }
  return [];
});

// SVG 圆环周长 (r = 22 -> 2 * pi * 22 ≈ 138.2)
const RING_CIRC = 138.2;
const cpuRingOffset = computed(() => {
  const pct = Math.min(100, Math.max(0, snap.value?.cpuUsagePct || 0));
  return RING_CIRC - (RING_CIRC * pct) / 100;
});

function diskPct(d: DiskUsageItem): number {
  if (!d.totalBytes) return 0;
  return Math.min(100, Math.round((d.usedBytes * 100) / d.totalBytes));
}

const normalizedBars = computed(() => {
  const list = netBars.value;
  if (!list.length) {
    return Array.from({ length: 20 }, () => ({ rxH: 8, txH: 6 }));
  }
  const maxVal = Math.max(1024, ...list.map((b) => Math.max(b.rx, b.tx)));
  const padded = [
    ...Array.from({ length: Math.max(0, 20 - list.length) }, () => ({ rx: 0, tx: 0 })),
    ...list.slice(-20),
  ];
  return padded.map((b) => ({
    rxH: Math.max(6, Math.round((b.rx / maxVal) * 100)),
    txH: Math.max(6, Math.round((b.tx / maxVal) * 100)),
  }));
});

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, i);
  return `${val >= 100 || i === 0 ? val.toFixed(0) : val.toFixed(1)} ${units[i]}`;
}

function formatRate(bps: number): string {
  if (!bps || bps <= 0) return '0 B/s';
  return `${formatBytes(bps)}/s`;
}

function formatUptime(secs: number): string {
  if (!secs || secs <= 0) return '--';
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}天 ${h}小时`;
  if (h > 0) return `${h}小时 ${m}分`;
  return `${m}分钟`;
}

async function runInspectOnce(hostId: string) {
  const seq = ++reqSeq;
  inspecting.value = true;
  try {
    const res = await fetch(`/api/hosts/${encodeURIComponent(hostId)}/inspect`);
    const data = await res.json();
    if (seq !== reqSeq || props.host.id !== hostId) return;
    if (!data.ok || !data.inspection) {
      inspectErr.value = data.error || '探索采集失败';
      return;
    }
    inspectErr.value = null;
    const ins = data.inspection as HostInspection;
    snap.value = ins;
    inspectionCache.set(hostId, ins);

    const hist = netHistoryCache.get(hostId) || [];
    hist.push({ rx: ins.netRxBps, tx: ins.netTxBps });
    if (hist.length > 20) hist.shift();
    netHistoryCache.set(hostId, hist);
    netBars.value = [...hist];

    if (data.host) {
      emit('hostUpdated', data.host as PublicHost);
    }
  } catch (e) {
    if (seq === reqSeq) {
      inspectErr.value = (e as Error).message;
    }
  } finally {
    if (seq === reqSeq) {
      inspecting.value = false;
    }
  }
}

function startLiveInspection(hostId: string) {
  stopLiveInspection();
  snap.value = inspectionCache.get(hostId) || null;
  netBars.value = [...(netHistoryCache.get(hostId) || [])];
  inspectErr.value = null;

  void runInspectOnce(hostId);

  inspectTimer = setInterval(() => {
    if (!inspecting.value && props.host.id === hostId) {
      void runInspectOnce(hostId);
    }
  }, 2800);
}

function stopLiveInspection() {
  if (inspectTimer) {
    clearInterval(inspectTimer);
    inspectTimer = null;
  }
}

const roles: Array<{ key: HostRole | ''; label: string }> = [
  { key: '', label: '未标记' },
  { key: 'primary', label: ROLE_META.primary.label },
  { key: 'proxy', label: ROLE_META.proxy.label },
  { key: 'test', label: ROLE_META.test.label },
  { key: 'archive', label: ROLE_META.archive.label },
];

function addTag() {
  const raw = tagInput.value.trim();
  if (!raw) return;
  const parts = raw
    .split(/[\s,，、]+/)
    .map((s) => s.trim())
    .filter(Boolean);
  const next = Array.from(new Set([...props.host.tags, ...parts]));
  tagInput.value = '';
  emit('patchMeta', props.host, { tags: next });
}

function removeTag(tag: string) {
  emit(
    'patchMeta',
    props.host,
    { tags: props.host.tags.filter((t) => t !== tag) }
  );
}

function setRole(role: HostRole | '') {
  emit('patchMeta', props.host, { role: role || null });
}

watch(
  () => props.host.id,
  (newId) => {
    startLiveInspection(newId);
  }
);

onMounted(() => {
  startLiveInspection(props.host.id);
});

onBeforeUnmount(() => {
  stopLiveInspection();
});
</script>

<template>
  <aside
    class="flex h-full w-full shrink-0 flex-col overflow-hidden rounded-2xl border border-zinc-200/80 bg-white/88 shadow-[0_4px_20px_rgba(0,0,0,0.03)] backdrop-blur-xl dark:border-zinc-800 dark:bg-zinc-900/88"
  >
    <!-- 顶部栏 -->
    <div class="flex items-center justify-between border-b border-zinc-100 px-4 py-3 dark:border-zinc-800">
      <div class="flex min-w-0 items-center gap-2">
        <span
          class="h-2.5 w-2.5 shrink-0 rounded-full"
          :class="{
            'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.55)]': host.status === 'online',
            'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.45)]': host.status === 'offline',
            'bg-zinc-300 dark:bg-zinc-600': host.status === 'unknown',
          }"
        />
        <h3 class="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">{{ host.name }}</h3>
        <span
          v-if="inspecting"
          class="h-1.5 w-1.5 animate-ping rounded-full bg-zinc-400"
          title="正在实时采样内核计数..."
        />
      </div>
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="rounded-lg px-2 py-1 text-xs font-medium text-zinc-600 transition hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
          :disabled="inspecting"
          @click="runInspectOnce(host.id)"
        >
          刷新
        </button>
        <button
          type="button"
          class="rounded-lg p-1 text-zinc-400 transition hover:bg-zinc-100 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
          title="收起详情"
          @click="emit('close')"
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
    </div>

    <!-- 主内容滚动区 -->
    <div class="flex-1 space-y-2.5 overflow-y-auto p-3.5">
      <!-- 1. 环境底座铭牌 -->
      <div class="rounded-xl border border-zinc-200/80 bg-white p-3 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-start gap-2.5">
          <OsBadge :os-info="host.osInfo" :kind="host.kind" :name="host.name" />
          <div class="min-w-0 flex-1">
            <div class="flex items-center justify-between gap-1">
              <span class="truncate text-xs font-semibold text-zinc-900 dark:text-zinc-100" :title="osPretty">
                {{ osPretty }}
              </span>
              <span
                v-if="snap?.tcpCc"
                class="shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase"
                :class="
                  snap.tcpCc.toLowerCase().includes('bbr')
                    ? 'bg-emerald-50 text-emerald-700 ring-1 ring-emerald-500/20 dark:bg-emerald-950/60 dark:text-emerald-400'
                    : 'bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300'
                "
              >
                {{ snap.tcpCc }}
              </span>
            </div>
            <p class="mt-0.5 truncate font-mono text-[11px] text-zinc-500 dark:text-zinc-400" :title="cpuLabel">
              {{ cpuLabel }}
            </p>
          </div>
        </div>

        <div class="mt-2.5 grid grid-cols-3 divide-x divide-zinc-100 border-t border-zinc-100 pt-2 text-center dark:divide-zinc-800 dark:border-zinc-800">
          <div>
            <div class="text-[10px] text-zinc-400">运行时间</div>
            <div class="mt-0.5 font-mono text-[11px] font-semibold text-zinc-800 dark:text-zinc-200">
              {{ snap ? formatUptime(snap.uptimeSeconds) : '--' }}
            </div>
          </div>
          <div>
            <div class="text-[10px] text-zinc-400">内核 / 架构</div>
            <div class="mt-0.5 truncate px-1 font-mono text-[11px] font-semibold text-zinc-800 dark:text-zinc-200" :title="host.osInfo?.kernel || ''">
              {{ (host.osInfo?.kernel || host.osInfo?.arch || '--').split('-')[0] }}
            </div>
          </div>
          <div>
            <div class="text-[10px] text-zinc-400">SSH 响应</div>
            <div class="mt-0.5 font-mono text-[11px] font-semibold text-zinc-800 dark:text-zinc-200">
              {{ host.latencyMs != null ? `${host.latencyMs} ms` : '--' }}
            </div>
          </div>
        </div>
      </div>

      <!-- 若探索报错 -->
      <div
        v-if="inspectErr && !snap"
        class="rounded-xl border border-red-200/80 bg-red-50/70 px-3 py-2 text-[11px] leading-relaxed text-red-700 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-400"
      >
        {{ inspectErr }}
      </div>

      <!-- 2. CPU 算力与负载 -->
      <div class="rounded-xl border border-zinc-200/80 bg-white p-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">CPU 算力与负载</span>
            <span class="rounded bg-zinc-100 px-1.5 py-0.2 font-mono text-[10px] text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
              {{ snap?.cpuCores || host.osInfo?.cpuCores || 1 }} 核
            </span>
          </div>
          <span
            v-if="snap"
            class="rounded px-1.5 py-0.2 font-mono text-[10px] font-medium"
            :class="
              snap.cpuStealPct >= 5
                ? 'bg-orange-100 text-orange-700 dark:bg-orange-950/60 dark:text-orange-400'
                : 'bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400'
            "
            title="宿主机邻居争抢/超售被偷走的 CPU 时间占比 (Steal)"
          >
            超售 Steal {{ snap.cpuStealPct.toFixed(1) }}%
          </span>
        </div>

        <div class="mt-2.5 flex items-center gap-3.5">
          <!-- 左侧 52px 圆环（自适应亮暗主题颜色） -->
          <div class="relative flex h-[52px] w-[52px] shrink-0 items-center justify-center">
            <svg class="h-[52px] w-[52px] -rotate-90" viewBox="0 0 52 52">
              <circle
                cx="26"
                cy="26"
                r="22"
                fill="none"
                class="stroke-zinc-100 dark:stroke-zinc-800"
                stroke-width="4.5"
              />
              <circle
                cx="26"
                cy="26"
                r="22"
                fill="none"
                :class="
                  (snap?.cpuUsagePct || 0) > 80
                    ? 'stroke-orange-500'
                    : 'stroke-zinc-900 dark:stroke-zinc-100'
                "
                stroke-width="4.5"
                stroke-linecap="round"
                :stroke-dasharray="RING_CIRC"
                :stroke-dashoffset="cpuRingOffset"
                class="transition-all duration-500"
              />
            </svg>
            <span class="font-mono text-xs font-bold text-zinc-900 dark:text-zinc-100">
              {{ snap ? `${snap.cpuUsagePct.toFixed(1)}%` : '--' }}
            </span>
          </div>

          <!-- 右侧宽敞指标矩阵 -->
          <div class="grid min-w-0 flex-1 grid-cols-2 gap-x-3 gap-y-1.5 rounded-lg bg-zinc-50/80 px-3 py-2 text-[11px] dark:bg-zinc-800/50">
            <div class="flex items-center justify-between">
              <span class="text-zinc-400">1分负载</span>
              <span class="font-mono font-semibold text-zinc-800 dark:text-zinc-200">{{ snap ? snap.load1.toFixed(2) : '--' }}</span>
            </div>
            <div class="flex items-center justify-between">
              <span class="text-zinc-400">5分负载</span>
              <span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{{ snap ? snap.load5.toFixed(2) : '--' }}</span>
            </div>
            <div class="flex items-center justify-between">
              <span class="text-zinc-400">15分负载</span>
              <span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{{ snap ? snap.load15.toFixed(2) : '--' }}</span>
            </div>
            <div class="flex items-center justify-between">
              <span class="text-zinc-400">IOWait</span>
              <span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{{ snap ? `${snap.cpuIowaitPct.toFixed(1)}%` : '--' }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 3. 内存与 Swap 解剖 -->
      <div class="rounded-xl border border-zinc-200/80 bg-white p-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-center justify-between text-xs">
          <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">内存结构 (RAM & Swap)</span>
          <span class="font-mono text-[11px] font-semibold text-zinc-900 dark:text-zinc-100">
            {{ snap ? `${formatBytes(snap.memUsedBytes)} / ${formatBytes(snap.memTotalBytes)}` : '-- / --' }}
          </span>
        </div>

        <div class="mt-2 flex h-2 w-full overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
          <div
            class="h-full bg-zinc-900 transition-all duration-500 dark:bg-zinc-100"
            :style="{ width: `${memUsedPct}%` }"
            title="应用真实占用 (MemTotal - MemAvailable)"
          />
          <div
            class="h-full bg-zinc-300 transition-all duration-500 dark:bg-zinc-600"
            :style="{ width: `${memCachePct}%` }"
            title="内核 Buffers/Cached（需要时可随时释放）"
          />
        </div>

        <div class="mt-2 flex items-center justify-between text-[11px] text-zinc-500 dark:text-zinc-400">
          <div class="flex items-center gap-3">
            <span class="inline-flex items-center gap-1">
              <span class="h-2 w-2 rounded-sm bg-zinc-900 dark:bg-zinc-100" />
              实占 {{ memUsedPct }}%
            </span>
            <span v-if="snap && snap.memCachedBytes > 0" class="inline-flex items-center gap-1">
              <span class="h-2 w-2 rounded-sm bg-zinc-300 dark:bg-zinc-600" />
              缓存 {{ formatBytes(snap.memCachedBytes) }}
            </span>
          </div>
          <span class="font-mono text-[10px] text-zinc-400">
            Swap:
            <template v-if="snap && snap.swapTotalBytes > 0">
              {{ formatBytes(snap.swapUsedBytes) }} / {{ formatBytes(snap.swapTotalBytes) }} ({{ swapUsedPct }}%)
            </template>
            <template v-else>未开启</template>
          </span>
        </div>
      </div>

      <!-- 4. 磁盘存储 -->
      <div class="rounded-xl border border-zinc-200/80 bg-white p-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-center justify-between">
          <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">磁盘存储</span>
          <span v-if="diskList.length === 1" class="font-mono text-[11px] font-semibold text-zinc-900 dark:text-zinc-100">
            {{ formatBytes(diskList[0].usedBytes) }} / {{ formatBytes(diskList[0].totalBytes) }}
          </span>
          <span v-else class="font-mono text-[10px] text-zinc-400">
            {{ diskList.length }} 个挂载分区
          </span>
        </div>

        <div v-if="diskList.length" class="mt-2.5 space-y-2.5">
          <div v-for="d in diskList" :key="d.mount" class="space-y-1.5">
            <div class="flex items-center justify-between font-mono text-[11px]">
              <span class="font-semibold text-zinc-800 dark:text-zinc-200">{{ d.mount }}</span>
              <span class="text-zinc-500 dark:text-zinc-400">
                {{ formatBytes(d.usedBytes) }} / {{ formatBytes(d.totalBytes) }}
                <span class="ml-1 font-semibold text-zinc-900 dark:text-zinc-100">({{ diskPct(d) }}%)</span>
              </span>
            </div>
            <div class="h-2 w-full overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
              <div
                class="h-full rounded-full transition-all duration-500"
                :class="diskPct(d) > 85 ? 'bg-red-500' : 'bg-zinc-900 dark:bg-zinc-100'"
                :style="{ width: `${diskPct(d)}%` }"
              />
            </div>
          </div>
        </div>
        <div v-else class="mt-2 text-xs text-zinc-400">正在探测磁盘容量...</div>
      </div>

      <!-- 5. 唯一外网主网卡实时脉冲谱 -->
      <div class="rounded-xl border border-zinc-200/80 bg-white p-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1.5">
            <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">网络吞吐</span>
            <span class="rounded bg-zinc-100 px-1.5 py-0.2 font-mono text-[10px] font-medium text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
              {{ snap?.netIface || 'eth0' }}
            </span>
          </div>
          <div class="flex items-center gap-2.5 font-mono text-[11px]">
            <span class="font-semibold text-emerald-600 dark:text-emerald-400">
              ↓ {{ snap ? formatRate(snap.netRxBps) : '0 B/s' }}
            </span>
            <span class="font-semibold text-zinc-800 dark:text-zinc-200">
              ↑ {{ snap ? formatRate(snap.netTxBps) : '0 B/s' }}
            </span>
          </div>
        </div>

        <div class="mt-2.5 flex h-12 items-end gap-1 rounded-lg bg-zinc-50/80 px-2 py-1.5 dark:bg-zinc-800/50">
          <div
            v-for="(bar, idx) in normalizedBars"
            :key="idx"
            class="flex h-full flex-1 flex-col justify-end gap-0.5"
          >
            <div
              class="w-full rounded-sm bg-zinc-800/75 transition-all duration-300 dark:bg-zinc-300/75"
              :style="{ height: `${Math.max(8, Math.round(bar.txH * 0.45))}%` }"
            />
            <div
              class="w-full rounded-sm bg-emerald-500/85 transition-all duration-300"
              :style="{ height: `${Math.max(8, Math.round(bar.rxH * 0.55))}%` }"
            />
          </div>
        </div>

        <div class="mt-2 flex items-center justify-between border-t border-zinc-100 pt-2 font-mono text-[10px] text-zinc-400 dark:border-zinc-800">
          <span>累计下行: <strong class="text-zinc-700 dark:text-zinc-200">{{ snap ? formatBytes(snap.netRxBytesTotal) : '--' }}</strong></span>
          <span>累计上行: <strong class="text-zinc-700 dark:text-zinc-200">{{ snap ? formatBytes(snap.netTxBytesTotal) : '--' }}</strong></span>
        </div>
      </div>

      <!-- 6. 角色、标签与快捷链接 -->
      <div class="space-y-3 rounded-xl border border-zinc-200/80 bg-white p-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.02)] dark:border-zinc-800 dark:bg-zinc-900/90">
        <div class="flex items-center justify-between">
          <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">角色分类</span>
          <span v-if="host.jumpHostName" class="rounded-md bg-zinc-100 px-1.5 py-0.5 text-[11px] text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
            跳板: {{ host.jumpHostName }}
          </span>
        </div>
        <div class="flex flex-wrap gap-1">
          <button
            v-for="r in roles"
            :key="r.key"
            type="button"
            class="rounded-lg px-2 py-1 text-[11px] font-medium transition"
            :class="
              (host.role || '') === r.key
                ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
                : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200/70 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700'
            "
            @click="setRole(r.key)"
          >
            {{ r.label }}
          </button>
        </div>

        <div class="pt-1">
          <div class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">标签</div>
          <div class="mt-1.5 flex flex-wrap gap-1.5">
            <span
              v-for="tag in host.tags"
              :key="tag"
              class="inline-flex items-center gap-1 rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 text-[11px] text-zinc-700 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
            >
              #{{ tag }}
              <button
                type="button"
                class="text-zinc-400 hover:text-zinc-800 dark:hover:text-zinc-100"
                @click="removeTag(tag)"
              >
                ×
              </button>
            </span>
          </div>
          <form class="mt-2 flex gap-1.5" @submit.prevent="addTag">
            <input
              v-model="tagInput"
              type="text"
              placeholder="添加标签（回车保存）"
              class="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-zinc-50/70 px-2.5 py-1 text-xs text-zinc-800 outline-none focus:border-zinc-400 focus:bg-white dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100"
            />
            <button
              type="submit"
              class="rounded-lg border border-zinc-200 bg-white px-2.5 py-1 text-xs font-medium text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-200"
            >
              添加
            </button>
          </form>
        </div>

        <div v-if="host.links.length" class="pt-1">
          <div class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">快捷链接</div>
          <div class="mt-1.5 space-y-1">
            <a
              v-for="link in host.links"
              :key="link.id || link.url"
              :href="link.url"
              target="_blank"
              rel="noreferrer"
              class="flex items-center justify-between rounded-lg border border-zinc-200/70 bg-zinc-50/60 px-2.5 py-1.5 text-xs text-zinc-700 transition hover:border-zinc-300 hover:bg-white dark:border-zinc-700 dark:bg-zinc-800/60 dark:text-zinc-200 dark:hover:bg-zinc-800"
            >
              <span class="truncate font-medium">{{ link.name }}</span>
              <span class="ml-2 truncate font-mono text-[10px] text-zinc-400">{{ link.url }}</span>
            </a>
          </div>
        </div>
      </div>
    </div>

    <!-- 底部操作栏 -->
    <div class="flex items-center justify-between border-t border-zinc-100 bg-zinc-50/60 px-3.5 py-2.5 dark:border-zinc-800 dark:bg-zinc-900/60">
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white"
          @click="emit('openTerminal', host)"
        >
          打开终端
        </button>
        <button
          type="button"
          class="rounded-lg border border-zinc-200 bg-white px-3 py-1.5 text-xs font-medium text-zinc-700 transition hover:bg-zinc-100 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700"
          @click="emit('openFiles', host)"
        >
          文件浏览
        </button>
      </div>
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="rounded-lg px-2.5 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-200/60 dark:text-zinc-400 dark:hover:bg-zinc-800"
          @click="emit('edit', host)"
        >
          编辑
        </button>
        <button
          v-if="host.kind !== 'local'"
          type="button"
          class="rounded-lg px-2 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950/40"
          @click="emit('delete', host)"
        >
          删除
        </button>
      </div>
    </div>
  </aside>
</template>
