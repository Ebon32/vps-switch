<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Terminal, type ITheme } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';
import type { PaneMeta } from '../types';

const props = defineProps<{
  paneId: string;
  dark: boolean;
  focused: boolean;
  showClose?: boolean;
}>();

const emit = defineEmits<{
  focus: [];
  closePane: [paneId: string];
  metaUpdate: [meta: PaneMeta];
  reconnect: [paneId: string];
}>();

const containerRef = ref<HTMLDivElement | null>(null);
const connected = ref(false);
const exited = ref(false);
const disconnected = ref(false);

let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let ws: WebSocket | null = null;
let resizeObserver: ResizeObserver | null = null;
let fitRaf: number | null = null;
let replayingDepth = 0;
let disposed = false;
let reconnectTimer: number | null = null;
let reconnectAttempts = 0;
const MAX_RECONNECT_ATTEMPTS = 6;
const textEncoder = new TextEncoder();

// 拦截 xterm.js 自动发出的设备属性应答（如 DA1 \x1b[?1;2c、DA2 \x1b[>0;276;0c），防止在 Shell 提示符上打出 [?1;2c
const AUTO_TERMINAL_RESPONSE_RE = /^\x1b\[[?>=][0-9;]*c$/;

// 清爽高对比白底主题（严格参考图 4 Reflex 白底终端）
const LIGHT_THEME: ITheme = {
  background: '#ffffff',
  foreground: '#18181b',
  cursor: '#18181b',
  cursorAccent: '#ffffff',
  selectionBackground: 'rgba(24, 24, 27, 0.14)',
  selectionForeground: '#18181b',
  black: '#18181b',
  red: '#dc2626',
  green: '#16a34a',
  yellow: '#ca8a04',
  blue: '#2563eb',
  magenta: '#9333ea',
  cyan: '#0891b2',
  white: '#e4e4e7',
  brightBlack: '#71717a',
  brightRed: '#ef4444',
  brightGreen: '#22c55e',
  brightYellow: '#eab308',
  brightBlue: '#3b82f6',
  brightMagenta: '#a855f7',
  brightCyan: '#06b6d4',
  brightWhite: '#fafafa',
};

// 经典深色终端主题（供一键切换）
const DARK_THEME: ITheme = {
  background: '#0c0d10',
  foreground: '#e4e4e7',
  cursor: '#f4f4f5',
  cursorAccent: '#0c0d10',
  selectionBackground: 'rgba(255, 255, 255, 0.18)',
  black: '#27272a',
  red: '#f87171',
  green: '#4ade80',
  yellow: '#facc15',
  blue: '#60a5fa',
  magenta: '#c084fc',
  cyan: '#22d3ee',
  white: '#e4e4e7',
  brightBlack: '#71717a',
  brightRed: '#fca5a5',
  brightGreen: '#86efac',
  brightYellow: '#fde047',
  brightBlue: '#93c5fd',
  brightMagenta: '#d8b4fe',
  brightCyan: '#67e8f9',
  brightWhite: '#ffffff',
};

function sendBinaryInput(data: string) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  const bytes = textEncoder.encode(data);
  const frame = new Uint8Array(1 + bytes.length);
  frame[0] = 0x01;
  frame.set(bytes, 1);
  ws.send(frame);
}

function sendControl(payload: Record<string, unknown>) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  ws.send(JSON.stringify(payload));
}

function scheduleFit() {
  if (fitRaf != null) cancelAnimationFrame(fitRaf);
  fitRaf = requestAnimationFrame(() => {
    fitRaf = null;
    if (!term || !fitAddon || !containerRef.value) return;
    if (containerRef.value.clientWidth < 40 || containerRef.value.clientHeight < 40) return;
    try {
      fitAddon.fit();
      if (term.cols >= 2 && term.rows >= 1) {
        sendControl({
          type: 'resize',
          cols: term.cols,
          rows: term.rows,
        });
      }
    } catch {
      // ignore transient layout errors
    }
  });
}

function connectWebSocket() {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const url = `${proto}//${window.location.host}/api/ws/terminal/${encodeURIComponent(props.paneId)}`;
  ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';

  ws.onopen = () => {
    connected.value = true;
    disconnected.value = false;
    reconnectAttempts = 0;
    scheduleFit();
  };

  ws.onmessage = (ev) => {
    if (ev.data instanceof ArrayBuffer) {
      const view = new Uint8Array(ev.data);
      if (view.length > 1 && view[0] === 0x00) {
        // 实时输出帧
        term?.write(view.subarray(1));
      } else if (view.length > 1 && view[0] === 0x02) {
        // 历史字节环回放帧：这是后端权威现场的整份快照，不是增量。
        // 触发点有两个——新建连接、以及客户端落后时被补发——两种情况终端里
        // 都已经有内容了，直接写进去会和旧内容叠加成重复、错乱的现场
        // （典型现象：切终端或断线重连后同一段内容出现两遍）。所以先整屏重置
        // 再写入；重置走写入队列，与实时帧保持先后顺序。
        if (term) {
          replayingDepth += 1;
          term.write('\x1bc');
          term.write(view.subarray(1), () => {
            replayingDepth = Math.max(0, replayingDepth - 1);
          });
        }
      }
      return;
    }
    if (typeof ev.data === 'string') {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.type === 'meta' && msg.pane) {
          exited.value = Boolean(msg.pane.exited);
          emit('metaUpdate', msg.pane as PaneMeta);
        }
      } catch {
        // ignore
      }
    }
  };

  ws.onclose = () => {
    connected.value = false;
    if (disposed) return;
    // 后端在新连接上会补发 0x02 历史字节环回放帧，重连即恢复现场；
    // 退避重试有上限，避免服务端已关闭该 pane 时无限重连。
    disconnected.value = true;
    if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) return;
    const delay = Math.min(400 * 2 ** reconnectAttempts, 4000);
    reconnectAttempts += 1;
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null;
      if (!disposed) connectWebSocket();
    }, delay);
  };
}

function manualReconnect() {
  if (disposed) return;
  if (reconnectTimer != null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  reconnectAttempts = 0;
  disconnected.value = false;
  connectWebSocket();
}

watch(
  () => props.dark,
  (isDark) => {
    if (term) {
      term.options.theme = isDark ? DARK_THEME : LIGHT_THEME;
    }
  }
);

onMounted(() => {
  if (!containerRef.value) return;

  term = new Terminal({
    cursorBlink: true,
    cursorStyle: 'block',
    fontFamily: '"JetBrains Mono", "Cascadia Mono", Consolas, "PingFang SC", "Microsoft YaHei", monospace',
    fontSize: 13.5,
    lineHeight: 1.32,
    scrollback: 5000,
    allowProposedApi: true,
    theme: props.dark ? DARK_THEME : LIGHT_THEME,
  });

  fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.loadAddon(new WebLinksAddon());

  term.open(containerRef.value);

  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      webgl.dispose();
    });
    term.loadAddon(webgl);
  } catch {
    // WebGL 不可用时自动退回 Canvas 渲染
  }

  // 智能快捷键：有选区时 Ctrl+C 复制；Ctrl+V 走安全 bracketed paste
  term.attachCustomKeyEventHandler((e: KeyboardEvent) => {
    if (e.type !== 'keydown') return true;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c' && !e.shiftKey && !e.altKey) {
      const sel = term?.getSelection();
      if (sel) {
        void navigator.clipboard.writeText(sel);
        term?.clearSelection();
        return false;
      }
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v' && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      void navigator.clipboard.readText().then((txt) => {
        if (txt) sendControl({ type: 'paste', text: txt });
      });
      return false;
    }
    return true;
  });

  term.onData((data) => {
    if (replayingDepth > 0) return;
    if (AUTO_TERMINAL_RESPONSE_RE.test(data)) return;
    sendBinaryInput(data);
  });

  resizeObserver = new ResizeObserver(() => {
    scheduleFit();
  });
  resizeObserver.observe(containerRef.value);

  connectWebSocket();
  term.focus();
});

onBeforeUnmount(() => {
  disposed = true;
  if (reconnectTimer != null) clearTimeout(reconnectTimer);
  if (fitRaf != null) cancelAnimationFrame(fitRaf);
  resizeObserver?.disconnect();
  if (ws) {
    ws.onclose = null;
    ws.close();
    ws = null;
  }
  term?.dispose();
  term = null;
});
</script>

<template>
  <div
    class="group/pane relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden transition-colors"
    :class="[
      dark ? 'bg-[#0c0d10]' : 'bg-white',
      focused ? 'ring-1 ring-inset ring-zinc-900/10' : '',
    ]"
    @mousedown="emit('focus')"
  >
    <!-- 分屏时的右上角悬浮关闭小按钮 -->
    <button
      v-if="showClose"
      type="button"
      class="absolute right-2.5 top-2 z-20 hidden rounded-lg border px-1.5 py-0.5 text-[11px] opacity-80 transition group-hover/pane:inline-flex"
      :class="
        dark
          ? 'border-zinc-700 bg-zinc-800 text-zinc-300 hover:bg-zinc-700'
          : 'border-zinc-200 bg-white/90 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-900'
      "
      title="关闭此分屏"
      @click.stop="emit('closePane', paneId)"
    >
      关闭分屏 ✕
    </button>

    <!-- xterm 挂载容器 -->
    <div ref="containerRef" class="h-full w-full flex-1 overflow-hidden px-3 py-2" />

    <!-- 连接断开提示条：后端支持字节环回放，重连即恢复现场 -->
    <div
      v-if="disconnected && !exited"
      class="flex items-center justify-between border-t border-amber-200 bg-amber-50 px-3 py-1.5 text-xs text-amber-700"
    >
      <span>{{
        reconnectAttempts >= MAX_RECONNECT_ATTEMPTS ? '终端连接已断开' : '连接中断，正在重连…'
      }}</span>
      <button
        type="button"
        class="rounded-md bg-amber-600 px-2 py-0.5 text-white hover:bg-amber-700"
        @click="manualReconnect"
      >
        立即重连
      </button>
    </div>

    <!-- 会话已结束提示条：远端断开或进程退出都走这里。SSH 掉线无法在字节层恢复，
         只能重建一个该主机的新会话（后端会在没有活终端时释放旧连接、重新登录）。 -->
    <div
      v-if="exited"
      class="flex items-center justify-between gap-2 border-t border-zinc-200/80 bg-zinc-50 px-3 py-1.5 text-xs text-zinc-500 dark:border-zinc-800 dark:bg-zinc-900/60 dark:text-zinc-400"
    >
      <span>终端会话已结束（远端断开或进程退出）</span>
      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="rounded-md bg-zinc-900 px-2 py-0.5 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white"
          title="为该主机重新建立一个终端会话"
          @click="emit('reconnect', paneId)"
        >
          重新连接
        </button>
        <button
          v-if="showClose"
          type="button"
          class="rounded-md border border-zinc-300 bg-white px-2 py-0.5 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
          @click="emit('closePane', paneId)"
        >
          移除面板
        </button>
      </div>
    </div>
  </div>
</template>
