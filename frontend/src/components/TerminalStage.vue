<script setup lang="ts">
import { computed, ref } from 'vue';
import TerminalPaneView from './TerminalPaneView.vue';
import type { PaneMeta, PublicHost } from '../types';

const props = defineProps<{
  host: PublicHost;
  panes: PaneMeta[];
  splitDirection: 'row' | 'col';
  dark: boolean;
  rightWing: 'detail' | 'files' | null;
  creating: boolean;
  standalone?: boolean;
}>();

const emit = defineEmits<{
  split: [direction: 'row' | 'col'];
  closePane: [paneId: string];
  reconnectPane: [paneId: string];
  toggleDark: [];
  toggleRightWing: [mode: 'files' | 'detail'];
  collapseStage: [];
  metaUpdate: [meta: PaneMeta];
  popout: [];
}>();

const focusedPaneId = ref<string | null>(null);

const activePane = computed(() => {
  return (
    props.panes.find((p) => p.id === focusedPaneId.value) ||
    props.panes[props.panes.length - 1] ||
    null
  );
});
</script>

<template>
  <section
    class="flex h-full w-full min-w-[420px] flex-col overflow-hidden rounded-2xl border border-zinc-200/80 bg-white/95 shadow-[0_4px_24px_rgba(0,0,0,0.04)] backdrop-blur-xl dark:border-zinc-800 dark:bg-zinc-900/95"
  >
    <!-- 舞台顶栏：主机状态 + OSC 7 实时目录 + OSC 133 命令状态 + 分屏/黑白底/右侧等宽栏开关 -->
    <div class="flex shrink-0 items-center justify-between gap-2 border-b border-zinc-100 px-4 py-2.5 dark:border-zinc-800">
      <!-- 左侧信息 -->
      <div class="flex min-w-0 items-center gap-2">
        <span
          class="h-2.5 w-2.5 shrink-0 rounded-full"
          :class="host.status === 'online' ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.5)]' : 'bg-zinc-300 dark:bg-zinc-600'"
        />
        <span class="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
          {{ host.name }}
        </span>
        <span class="hidden sm:inline font-mono text-xs text-zinc-400">
          {{ host.kind === 'local' ? 'PowerShell (ConPTY)' : `${host.username}@${host.addr}:${host.port}` }}
        </span>

        <!-- OSC 7 实时工作目录胶囊 -->
        <span
          v-if="activePane?.cwd"
          class="inline-flex max-w-[200px] items-center gap-1 truncate rounded-lg border border-zinc-200/80 bg-zinc-50 px-2 py-0.5 font-mono text-[11px] text-zinc-600 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
          :title="`当前目录: ${activePane.cwd}`"
        >
          <svg class="h-3 w-3 shrink-0 text-zinc-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
          </svg>
          <span class="truncate">{{ activePane.cwd }}</span>
        </span>

        <!-- OSC 133 实时命令运行态与真实 Exit Code -->
        <span
          v-if="activePane?.running"
          class="inline-flex items-center gap-1 rounded-lg bg-amber-50 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:bg-amber-950/60 dark:text-amber-400"
        >
          <span class="h-1.5 w-1.5 animate-ping rounded-full bg-amber-500" />
          运行中
        </span>
        <span
          v-else-if="activePane?.lastExitCode != null"
          class="inline-flex items-center rounded-lg px-1.5 py-0.5 font-mono text-[10px] font-medium"
          :class="
            activePane.lastExitCode === 0
              ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/60 dark:text-emerald-400'
              : 'bg-red-50 text-red-600 dark:bg-red-950/60 dark:text-red-400'
          "
          :title="`上一条命令退出码: ${activePane.lastExitCode}`"
        >
          {{ activePane.lastExitCode === 0 ? '✓ 0' : `✗ ${activePane.lastExitCode}` }}
        </span>
      </div>

      <!-- 右侧工具条：分屏 + 主题 + 右侧等宽栏（文件管理 / 详情图表） + 收起 -->
      <div class="flex shrink-0 items-center gap-1.5">
        <!-- 左右分屏 / 上下分屏 -->
        <div class="flex items-center rounded-xl border border-zinc-200/80 bg-zinc-50/80 p-0.5 dark:border-zinc-700 dark:bg-zinc-800">
          <button
            type="button"
            class="rounded-lg px-2 py-1 text-[11px] font-medium text-zinc-600 transition hover:bg-white hover:text-zinc-900 hover:shadow-sm disabled:opacity-40 dark:text-zinc-300 dark:hover:bg-zinc-700 dark:hover:text-white"
            :disabled="creating"
            title="向右分屏（复用当前 SSH 连接）"
            @click="emit('split', 'row')"
          >
            左右分屏
          </button>
          <button
            type="button"
            class="rounded-lg px-2 py-1 text-[11px] font-medium text-zinc-600 transition hover:bg-white hover:text-zinc-900 hover:shadow-sm disabled:opacity-40 dark:text-zinc-300 dark:hover:bg-zinc-700 dark:hover:text-white"
            :disabled="creating"
            title="向下分屏（复用当前 SSH 连接）"
            @click="emit('split', 'col')"
          >
            上下分屏
          </button>
        </div>

        <!-- 白底 / 黑底一键切换 -->
        <button
          type="button"
          class="rounded-xl border border-zinc-200/80 bg-white px-2.5 py-1 text-xs font-medium text-zinc-600 transition hover:bg-zinc-100 hover:text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
          :title="dark ? '切换为清爽白底终端' : '切换为深色黑底终端'"
          @click="emit('toggleDark')"
        >
          {{ dark ? '白底' : '黑底' }}
        </button>

        <!-- 在最右栏展开与左栏等宽(336px)的「文件管理」 -->
        <button
          v-if="!standalone"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border px-2.5 py-1 text-xs font-medium transition"
          :class="
            rightWing === 'files'
              ? 'border-zinc-900 bg-zinc-900 text-white shadow-sm dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900'
              : 'border-zinc-200/80 bg-white text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700'
          "
          title="在右侧等宽栏展开文件管理"
          @click="emit('toggleRightWing', 'files')"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
          </svg>
          <span>文件管理</span>
        </button>

        <!-- 在最右栏展开与左栏等宽(336px)的「详情图表」 -->
        <button
          v-if="!standalone"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border px-2.5 py-1 text-xs font-medium transition"
          :class="
            rightWing === 'detail'
              ? 'border-zinc-900 bg-zinc-900 text-white shadow-sm dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900'
              : 'border-zinc-200/80 bg-white text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700'
          "
          title="在右侧等宽栏展开详情图表"
          @click="emit('toggleRightWing', 'detail')"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 20V10M12 20V4M6 20v-6" />
          </svg>
          <span>详情图表</span>
        </button>

        <!-- 弹出系统独立原生窗口 -->
        <button
          v-if="!standalone"
          type="button"
          class="inline-flex items-center gap-1 rounded-xl border border-zinc-200/80 bg-white px-2.5 py-1 text-xs font-medium text-zinc-600 transition hover:bg-zinc-100 hover:text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
          title="在独立的系统原生窗口中打开该终端"
          @click="emit('popout')"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14L21 3" />
          </svg>
          <span class="hidden sm:inline">独立窗口</span>
        </button>

        <button
          v-if="!standalone"
          type="button"
          class="ml-0.5 rounded-lg p-1 text-zinc-400 transition hover:bg-zinc-100 hover:text-zinc-800 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
          title="收起舞台回到仓库（终端后台保活，可从左上角标签秒切回）"
          @click="emit('collapseStage')"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- 终端分屏画布区 -->
    <div
      v-if="panes.length"
      class="flex min-h-0 flex-1 divide-zinc-200/80 overflow-hidden"
      :class="splitDirection === 'col' ? 'flex-col divide-y' : 'flex-row divide-x'"
    >
      <div
        v-for="pane in panes"
        :key="pane.id"
        class="min-h-0 min-w-0 flex-1 overflow-hidden"
      >
        <TerminalPaneView
          :pane-id="pane.id"
          :dark="dark"
          :focused="focusedPaneId === pane.id || panes.length === 1"
          :show-close="panes.length > 1"
          @focus="focusedPaneId = pane.id"
          @close-pane="emit('closePane', $event)"
          @reconnect="emit('reconnectPane', $event)"
          @meta-update="emit('metaUpdate', $event)"
        />
      </div>
    </div>

    <!-- 正在握手建立连接时的加载态 -->
    <div
      v-else
      class="flex flex-1 flex-col items-center justify-center gap-2 text-xs text-zinc-400"
    >
      <div class="h-5 w-5 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-800" />
      <span>正在连接 {{ host.name }} 终端...</span>
    </div>
  </section>
</template>
