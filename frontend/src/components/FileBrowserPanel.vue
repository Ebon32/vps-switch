<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { FileEntry, PublicHost, TransferTask } from '../types';

// CodeMirror 内核约 850KB，只有真要编辑文件时才值得加载，不拖累首屏
const CodeEditor = defineAsyncComponent(() => import('./CodeEditor.vue'));

interface DirCacheItem {
  path: string;
  parent: string | null;
  items: FileEntry[];
  ts: number;
}

interface BreadcrumbSegment {
  label: string;
  path: string;
}

const ARCHIVE_SUFFIXES = [
  '.tar.gz',
  '.tgz',
  '.tar.xz',
  '.txz',
  '.tar.bz2',
  '.tbz2',
  '.tar',
  '.zip',
  '.gz',
];

function isArchiveFile(item: FileEntry): boolean {
  if (item.isDir) return false;
  const lower = item.name.toLowerCase();
  return ARCHIVE_SUFFIXES.some((s) => lower.endsWith(s));
}

// 全局内存目录缓存
const globalDirCache = new Map<string, DirCacheItem>();
const inflightPrefetch = new Set<string>();

function makeCacheKey(hostId: string, path: string): string {
  return `${hostId}::${path || '~'}`;
}

const props = defineProps<{
  host: PublicHost;
  compact?: boolean;
  terminalCwd?: string | null;
  rightWing?: 'detail' | 'files' | null;
}>();

const emit = defineEmits<{
  close: [];
  expandFull: [];
  toggleDetail: [];
  toast: [text: string, kind?: 'ok' | 'err'];
}>();

const currentPath = ref('');
const pathInput = ref('');
const editingPath = ref(false);
const pathInputEl = ref<HTMLInputElement | null>(null);
const parentPath = ref<string | null>(null);
const items = ref<FileEntry[]>([]);
const loading = ref(false);
const busyActionPath = ref<string | null>(null);
const highlightedPath = ref<string | null>(null);
const listContainerRef = ref<HTMLDivElement | null>(null);
const search = ref('');
const showHidden = ref(true);
const dragActive = ref(false);
const fileInputRef = ref<HTMLInputElement | null>(null);

let navSeq = 0;
let activeAbort: AbortController | null = null;
let hoverTimer: ReturnType<typeof setTimeout> | null = null;
let highlightTimer: ReturnType<typeof setTimeout> | null = null;

function focusAndHighlightItem(targetPath: string | null | undefined) {
  if (!targetPath) return;
  const norm = targetPath.replace(/\\/g, '/');
  highlightedPath.value = norm;
  if (highlightTimer) clearTimeout(highlightTimer);
  highlightTimer = setTimeout(() => {
    if (highlightedPath.value === norm) {
      highlightedPath.value = null;
    }
  }, 4000);

  void nextTick(() => {
    const container = listContainerRef.value;
    if (!container) return;
    const rows = container.querySelectorAll<HTMLElement>('[data-entry-path]');
    for (const el of rows) {
      if (el.dataset.entryPath === norm) {
        el.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        break;
      }
    }
  });
}

// 在线文件编辑状态
const editorOpen = ref(false);
const editorPath = ref('');
const editorName = ref('');
const editorContent = ref('');
const editorLoading = ref(false);
const editorSaving = ref(false);

// 新建/重命名/删除确认条 / 下载落点
const inlinePrompt = ref<{
  kind: 'mkdir' | 'touch' | 'rename' | 'delete' | 'download';
  title: string;
  value: string;
  target?: FileEntry;
} | null>(null);

// 实时传输队列
const transfers = ref<TransferTask[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;

// 可点击的面包屑分段：如 /root/123 -> [/] [root] [123]；本机 Windows -> [此电脑] [E:] [1shell4.7]
const breadcrumbs = computed<BreadcrumbSegment[]>(() => {
  const p = currentPath.value.trim();
  const isLocalWin = props.host.kind === 'local';

  if (isLocalWin) {
    const crumbs: BreadcrumbSegment[] = [{ label: '此电脑', path: 'drives' }];
    if (!p || p === 'drives') return crumbs;
    const norm = p.replace(/\\/g, '/').replace(/\/+$/, '');
    const parts = norm.split('/').filter(Boolean);
    let acc = '';
    for (let i = 0; i < parts.length; i++) {
      const seg = parts[i];
      if (i === 0 && seg.endsWith(':')) {
        acc = `${seg}/`;
        crumbs.push({ label: seg, path: acc });
      } else {
        acc = acc.endsWith('/') ? `${acc}${seg}` : `${acc}/${seg}`;
        crumbs.push({ label: seg, path: acc });
      }
    }
    return crumbs;
  }

  // Linux / POSIX 路径
  const crumbs: BreadcrumbSegment[] = [{ label: '/', path: '/' }];
  if (!p || p === '/') return crumbs;
  const parts = p.split('/').filter(Boolean);
  let acc = '';
  for (const seg of parts) {
    acc = `${acc}/${seg}`;
    crumbs.push({ label: seg, path: acc });
  }
  return crumbs;
});

const visibleItems = computed(() => {
  const q = search.value.trim().toLowerCase();
  return items.value.filter((item) => {
    if (!showHidden.value && item.name.startsWith('.')) return false;
    if (!q) return true;
    return item.name.toLowerCase().includes(q);
  });
});

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, i);
  return `${val >= 100 || i === 0 ? val.toFixed(0) : val.toFixed(1)} ${units[i]}`;
}

function formatSpeed(bps: number): string {
  if (!bps || bps <= 0) return '计算中...';
  return `${formatBytes(bps)}/s`;
}

function formatTime(ms: number): string {
  if (!ms) return '';
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function joinPath(dir: string, name: string): string {
  const cleanDir = dir.replace(/\/+$/, '');
  if (!cleanDir || cleanDir === '/') return `/${name}`;
  return `${cleanDir}/${name}`;
}

function computeOptimisticParent(p: string): string | null {
  if (!p || p === '/' || p === 'drives') return null;
  if (/^[A-Za-z]:\/?$/.test(p)) return 'drives';
  const idx = p.lastIndexOf('/');
  if (idx < 0) return null;
  if (idx === 0) return '/';
  const sub = p.slice(0, idx);
  return /^[A-Za-z]:$/.test(sub) ? `${sub}/` : sub;
}

function storeDirCache(hostId: string, requestedPath: string, data: DirCacheItem) {
  globalDirCache.set(makeCacheKey(hostId, data.path), data);
  if (requestedPath !== data.path) {
    globalDirCache.set(makeCacheKey(hostId, requestedPath), data);
  }
}

async function prefetchDir(targetPath: string | null | undefined) {
  if (!targetPath) return;
  const hostId = props.host.id;
  const key = makeCacheKey(hostId, targetPath);
  const existing = globalDirCache.get(key);
  if (existing && Date.now() - existing.ts < 30_000) return;
  if (inflightPrefetch.has(key)) return;

  inflightPrefetch.add(key);
  try {
    const url = `/api/files/list?hostId=${encodeURIComponent(hostId)}&path=${encodeURIComponent(targetPath)}`;
    const res = await fetch(url);
    const data = await res.json();
    if (data.ok) {
      storeDirCache(hostId, targetPath, {
        path: data.path,
        parent: data.parent || null,
        items: data.items || [],
        ts: Date.now(),
      });
    }
  } catch {
    // ignore
  } finally {
    inflightPrefetch.delete(key);
  }
}

function onRowMouseEnter(item: FileEntry) {
  if (!item.isDir) return;
  if (hoverTimer) clearTimeout(hoverTimer);
  hoverTimer = setTimeout(() => {
    void prefetchDir(item.path);
  }, 45);
}

function onRowMouseLeave() {
  if (hoverTimer) {
    clearTimeout(hoverTimer);
    hoverTimer = null;
  }
}

function invalidateCache(hostId: string, path: string) {
  globalDirCache.delete(makeCacheKey(hostId, path));
}

async function startEditPath() {
  pathInput.value = currentPath.value === 'drives' ? '' : currentPath.value;
  editingPath.value = true;
  await nextTick();
  pathInputEl.value?.focus();
  pathInputEl.value?.select();
}

function submitPathInput() {
  editingPath.value = false;
  const target = pathInput.value.trim();
  if (target && target !== currentPath.value) {
    void loadDir(target);
  }
}

async function loadDir(
  targetPath?: string,
  opts: { forceRefresh?: boolean; silentKeepList?: boolean } = {}
) {
  const hostId = props.host.id;
  const reqPath = (targetPath !== undefined ? targetPath : currentPath.value).trim();
  const isSameDir = reqPath === currentPath.value && items.value.length > 0;
  const seq = ++navSeq;

  if (activeAbort) {
    activeAbort.abort();
    activeAbort = null;
  }

  editingPath.value = false;
  if (!isSameDir) {
    search.value = '';
    highlightedPath.value = null;
  }
  inlinePrompt.value = null;

  if (reqPath && !isSameDir) {
    currentPath.value = reqPath;
    pathInput.value = reqPath;
    parentPath.value = computeOptimisticParent(reqPath);
  }

  const cached = !opts.forceRefresh ? globalDirCache.get(makeCacheKey(hostId, reqPath)) : undefined;
  if (cached) {
    currentPath.value = cached.path;
    pathInput.value = cached.path;
    parentPath.value = cached.parent;
    items.value = cached.items;
    loading.value = false;
    if (Date.now() - cached.ts < 15_000) {
      if (cached.parent) void prefetchDir(cached.parent);
      return;
    }
  } else if (!isSameDir && !opts.silentKeepList) {
    // 只有跨目录跳转且无缓存时才清空显示骨架屏；同目录刷新（如压缩/解压/新建/改名后）保持原列表原地无感更新，绝不闪屏！
    items.value = [];
    loading.value = true;
  } else {
    loading.value = true;
  }

  const controller = new AbortController();
  activeAbort = controller;

  try {
    const refreshParam = opts.forceRefresh ? '&refresh=true' : '';
    const url = `/api/files/list?hostId=${encodeURIComponent(hostId)}&path=${encodeURIComponent(reqPath)}${refreshParam}`;
    const res = await fetch(url, { signal: controller.signal });
    const data = await res.json();
    if (seq !== navSeq) return;

    if (!data.ok) {
      emit('toast', data.error || '读取目录失败', 'err');
      loading.value = false;
      return;
    }

    const entry: DirCacheItem = {
      path: data.path,
      parent: data.parent || null,
      items: data.items || [],
      ts: Date.now(),
    };
    storeDirCache(hostId, reqPath, entry);

    currentPath.value = entry.path;
    pathInput.value = entry.path;
    parentPath.value = entry.parent;
    items.value = entry.items;
    loading.value = false;

    if (entry.parent) {
      void prefetchDir(entry.parent);
    }
  } catch (e) {
    if ((e as Error).name === 'AbortError') return;
    if (seq !== navSeq) return;
    loading.value = false;
    emit('toast', `读取目录失败: ${(e as Error).message}`, 'err');
  }
}

// 压缩文件或文件夹
async function handleCompress(item: FileEntry) {
  if (busyActionPath.value) return;
  busyActionPath.value = item.path;
  emit('toast', `正在压缩 ${item.name}...`);
  try {
    const res = await fetch('/api/files/compress', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        hostId: props.host.id,
        path: item.path,
      }),
    });
    const data = await res.json();
    if (!data.ok) {
      emit('toast', data.error || '压缩失败', 'err');
      return;
    }
    invalidateCache(props.host.id, currentPath.value);
    await loadDir(currentPath.value, { forceRefresh: true, silentKeepList: true });
    focusAndHighlightItem(data.path);
    emit('toast', `已压缩生成 ${data.path.split('/').pop()}`);
  } catch (e) {
    emit('toast', `压缩失败: ${(e as Error).message}`, 'err');
  } finally {
    busyActionPath.value = null;
  }
}

// 解压归档文件
async function handleExtract(item: FileEntry) {
  if (busyActionPath.value) return;
  busyActionPath.value = item.path;
  emit('toast', `正在解压 ${item.name}...`);
  try {
    const res = await fetch('/api/files/extract', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        hostId: props.host.id,
        path: item.path,
      }),
    });
    const data = await res.json();
    if (!data.ok) {
      emit('toast', data.error || '解压失败', 'err');
      return;
    }
    invalidateCache(props.host.id, currentPath.value);
    await loadDir(currentPath.value, { forceRefresh: true, silentKeepList: true });
    focusAndHighlightItem(data.path);
    emit('toast', `已解压至 ${data.path.split('/').pop()}`);
  } catch (e) {
    emit('toast', `解压失败: ${(e as Error).message}`, 'err');
  } finally {
    busyActionPath.value = null;
  }
}

async function fetchTransfers() {
  try {
    const res = await fetch(`/api/transfers?hostId=${encodeURIComponent(props.host.id)}`);
    const data = await res.json();
    if (data.ok && Array.isArray(data.tasks)) {
      transfers.value = data.tasks;
      const hasRunning = data.tasks.some((t: TransferTask) => t.state === 'running');
      if (!hasRunning && pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
    }
  } catch {
    // ignore
  }
}

function ensureTransferPolling() {
  void fetchTransfers();
  if (pollTimer) return;
  pollTimer = setInterval(() => {
    void fetchTransfers();
  }, 350);
}

async function clearCompletedTransfers() {
  await fetch('/api/transfers', { method: 'DELETE' });
  await fetchTransfers();
}

function onEntryClick(item: FileEntry) {
  if (item.isDir) {
    void loadDir(item.path);
  } else {
    void openFileEditor(item);
  }
}

async function openFileEditor(item: FileEntry) {
  editorOpen.value = true;
  editorPath.value = item.path;
  editorName.value = item.name;
  editorContent.value = '';
  editorLoading.value = true;
  try {
    const url = `/api/files/read?hostId=${encodeURIComponent(props.host.id)}&path=${encodeURIComponent(item.path)}`;
    const res = await fetch(url);
    const data = await res.json();
    if (!data.ok) {
      emit('toast', data.error || '打开文件失败', 'err');
      editorOpen.value = false;
      return;
    }
    editorContent.value = data.content || '';
  } catch (e) {
    emit('toast', `读取文件失败: ${(e as Error).message}`, 'err');
    editorOpen.value = false;
  } finally {
    editorLoading.value = false;
  }
}

async function saveFileEditor() {
  if (!editorPath.value || editorSaving.value) return;
  editorSaving.value = true;
  try {
    const res = await fetch('/api/files/write', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        hostId: props.host.id,
        path: editorPath.value,
        content: editorContent.value,
      }),
    });
    const data = await res.json();
    if (!data.ok) {
      emit('toast', data.error || '保存失败', 'err');
      return;
    }
    emit('toast', `已保存 ${editorName.value}`);
    invalidateCache(props.host.id, currentPath.value);
    void loadDir(currentPath.value, { forceRefresh: true });
  } catch (e) {
    emit('toast', `保存失败: ${(e as Error).message}`, 'err');
  } finally {
    editorSaving.value = false;
  }
}

function triggerDownload(item: FileEntry) {
  if (item.isDir) return;
  // 改走带断点续传的传输引擎（见 openInlineAction('download')），不再用浏览器直下
  openInlineAction('download', item);
}

function copyEntryPath(path: string) {
  void navigator.clipboard.writeText(path);
  emit('toast', `已复制路径: ${path}`);
}

function triggerUploadPicker() {
  fileInputRef.value?.click();
}

async function uploadFiles(fileList: FileList | File[]) {
  const arr = Array.from(fileList);
  if (!arr.length || !currentPath.value || currentPath.value === 'drives') return;
  ensureTransferPolling();

  for (const file of arr) {
    const q = new URLSearchParams({
      hostId: props.host.id,
      dirPath: currentPath.value,
      filename: file.name,
      size: String(file.size),
    });
    try {
      const res = await fetch(`/api/files/upload?${q.toString()}`, {
        method: 'POST',
        body: file,
      });
      const data = await res.json();
      if (!data.ok) {
        emit('toast', `${file.name} 上传失败: ${data.error}`, 'err');
      } else {
        emit('toast', `${file.name} 上传完成`);
      }
    } catch (e) {
      emit('toast', `${file.name} 上传失败: ${(e as Error).message}`, 'err');
    }
  }
  invalidateCache(props.host.id, currentPath.value);
  await fetchTransfers();
  await loadDir(currentPath.value, { forceRefresh: true });
}

function onFileInputChange(e: Event) {
  const input = e.target as HTMLInputElement;
  if (input.files && input.files.length) {
    void uploadFiles(input.files);
    input.value = '';
  }
}

function onDropFiles(e: DragEvent) {
  e.preventDefault();
  dragActive.value = false;
  if (e.dataTransfer?.files?.length) {
    void uploadFiles(e.dataTransfer.files);
  }
}

function openInlineAction(
  kind: 'mkdir' | 'touch' | 'rename' | 'delete' | 'download',
  target?: FileEntry
) {
  if (kind === 'mkdir') {
    inlinePrompt.value = { kind, title: '新建文件夹名称', value: '' };
  } else if (kind === 'touch') {
    inlinePrompt.value = { kind, title: '新建文件名称', value: '' };
  } else if (kind === 'rename' && target) {
    inlinePrompt.value = { kind, title: `重命名 ${target.name}`, value: target.name, target };
  } else if (kind === 'delete' && target) {
    inlinePrompt.value = {
      kind,
      title: `确认删除${target.isDir ? '目录' : '文件'}「${target.name}」？`,
      value: target.name,
      target,
    };
  } else if (kind === 'download' && target) {
    // 走带断点续传 + SHA-256 校验的传输引擎（不再是浏览器直下、断了只能从头）。
    // 留空则落到系统下载目录。
    inlinePrompt.value = {
      kind,
      title: `下载「${target.name}」到本机路径（留空 = 系统下载目录）`,
      value: '',
      target,
    };
  }
}

async function submitInlineAction() {
  const act = inlinePrompt.value;
  if (!act) return;
  const val = act.value.trim();

  let highlightTarget: string | null = null;
  try {
    if (act.kind === 'mkdir') {
      if (!val) return;
      const target = joinPath(currentPath.value, val);
      const res = await fetch('/api/files/mkdir', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostId: props.host.id,
          path: target,
        }),
      });
      const data = await res.json();
      if (!data.ok) throw new Error(data.error);
      highlightTarget = target;
      emit('toast', `已创建目录 ${val}`);
    } else if (act.kind === 'touch') {
      if (!val) return;
      const newPath = joinPath(currentPath.value, val);
      const res = await fetch('/api/files/write', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostId: props.host.id,
          path: newPath,
          content: '',
        }),
      });
      const data = await res.json();
      if (!data.ok) throw new Error(data.error);
      highlightTarget = newPath;
      emit('toast', `已创建文件 ${val}`);
    } else if (act.kind === 'rename' && act.target) {
      if (!val || val === act.target.name) {
        inlinePrompt.value = null;
        return;
      }
      const newPath = joinPath(currentPath.value, val);
      const res = await fetch('/api/files/rename', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostId: props.host.id,
          oldPath: act.target.path,
          newPath,
        }),
      });
      const data = await res.json();
      if (!data.ok) throw new Error(data.error);
      highlightTarget = newPath;
      emit('toast', `已重命名为 ${val}`);
    } else if (act.kind === 'delete' && act.target) {
      const res = await fetch('/api/files/remove', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostId: props.host.id,
          path: act.target.path,
          isDir: act.target.isDir,
        }),
      });
      const data = await res.json();
      if (!data.ok) throw new Error(data.error);
      if (editorPath.value === act.target.path) {
        editorOpen.value = false;
      }
      emit('toast', `已删除 ${act.target.name}`);
    } else if (act.kind === 'download' && act.target) {
      const res = await fetch('/api/transfers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          hostId: props.host.id,
          direction: 'download',
          localPath: val,
          remotePath: act.target.path,
        }),
      });
      const data = await res.json();
      if (!data.ok) throw new Error(data.error || '发起下载失败');
      inlinePrompt.value = null;
      ensureTransferPolling();
      emit(
        'toast',
        `开始下载到 ${data.localPath || '系统下载目录'}（队列中看进度；中断后重下会自动续传）`
      );
      return;
    }
    inlinePrompt.value = null;
    invalidateCache(props.host.id, currentPath.value);
    await loadDir(currentPath.value, { forceRefresh: true, silentKeepList: true });
    if (highlightTarget) {
      focusAndHighlightItem(highlightTarget);
    }
  } catch (e) {
    emit('toast', (e as Error).message || '操作失败', 'err');
  }
}

watch(
  () => props.host.id,
  () => {
    editorOpen.value = false;
    items.value = [];
    currentPath.value = props.terminalCwd || '';
    pathInput.value = currentPath.value;
    void loadDir(currentPath.value);
    void fetchTransfers();
  }
);

onMounted(() => {
  currentPath.value = props.terminalCwd || '';
  pathInput.value = currentPath.value;
  void loadDir(currentPath.value);
  void fetchTransfers();
});

onBeforeUnmount(() => {
  if (activeAbort) activeAbort.abort();
  if (hoverTimer) clearTimeout(hoverTimer);
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
});
</script>

<template>
  <section
    class="relative flex h-full w-full flex-col overflow-hidden rounded-2xl border border-zinc-200/80 bg-white/90 shadow-[0_4px_20px_rgba(0,0,0,0.03)] backdrop-blur-xl dark:border-zinc-800 dark:bg-zinc-900/90 [&_div.bg-white]:dark:bg-zinc-900 [&_input.bg-white]:dark:bg-zinc-800 [&_input.bg-white]:dark:border-zinc-700 [&_input.bg-white]:dark:text-zinc-100 [&_button.bg-white]:dark:bg-zinc-800 [&_button.bg-white]:dark:border-zinc-700 [&_button.bg-white]:dark:text-zinc-200 [&_span.text-zinc-800]:dark:text-zinc-200 [&_span.text-zinc-900]:dark:text-zinc-100"
    @dragover.prevent="dragActive = true"
    @dragleave.self="dragActive = false"
    @drop="onDropFiles"
  >
    <input
      ref="fileInputRef"
      type="file"
      multiple
      class="hidden"
      @change="onFileInputChange"
    />

    <!-- 拖拽上传高亮遮罩 -->
    <div
      v-if="dragActive"
      class="pointer-events-none absolute inset-2 z-40 flex flex-col items-center justify-center rounded-xl border-2 border-dashed border-zinc-900/40 bg-white/90 backdrop-blur-sm"
    >
      <p class="text-sm font-semibold text-zinc-900">松开鼠标直接满速上传至当前目录</p>
      <p class="mt-1 font-mono text-xs text-zinc-500">{{ currentPath }}</p>
    </div>

    <!-- 顶部栏 -->
    <div class="flex shrink-0 items-center justify-between gap-2 border-b border-zinc-100 px-3.5 py-2.5">
      <div class="flex min-w-0 items-center gap-2">
        <svg class="h-4 w-4 shrink-0 text-zinc-700" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
        </svg>
        <span class="truncate text-sm font-semibold text-zinc-900">
          {{ compact ? `${host.name} · 文件` : `${host.name} · 文件管理器` }}
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-1">
        <button
          v-if="terminalCwd && terminalCwd !== currentPath"
          type="button"
          class="rounded-lg border border-zinc-200/80 bg-zinc-50 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900"
          :title="`跳转到终端当前目录: ${terminalCwd}`"
          @click="loadDir(terminalCwd)"
        >
          同步终端目录
        </button>
        <button
          v-if="compact"
          type="button"
          class="rounded-lg px-2 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900"
          title="在主舞台全宽展开文件浏览器"
          @click="emit('expandFull')"
        >
          全屏展开
        </button>
        <button
          v-if="!compact"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border px-2.5 py-1 text-xs font-medium transition"
          :class="
            rightWing === 'detail'
              ? 'border-zinc-900 bg-zinc-900 text-white shadow-sm'
              : 'border-zinc-200/80 bg-white text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900'
          "
          @click="emit('toggleDetail')"
        >
          <span>详情图表</span>
        </button>
        <button
          type="button"
          class="rounded-lg p-1 text-zinc-400 hover:bg-zinc-100 hover:text-zinc-700"
          @click="emit('close')"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- 工具条：上级按钮 + 刷新 + 可点击面包屑路径栏 + 上传 -->
    <div class="flex shrink-0 flex-col gap-2 border-b border-zinc-100 bg-zinc-50/50 px-3 py-2">
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="rounded-lg border border-zinc-200/80 bg-white p-1.5 text-zinc-600 hover:bg-zinc-100 disabled:opacity-40"
          :disabled="!parentPath"
          title="返回上级目录"
          @mouseenter="prefetchDir(parentPath)"
          @click="parentPath && loadDir(parentPath)"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 15l-6-6-6 6" />
          </svg>
        </button>
        <button
          type="button"
          class="rounded-lg border border-zinc-200/80 bg-white p-1.5 text-zinc-600 hover:bg-zinc-100"
          title="强制刷新当前目录"
          @click="loadDir(currentPath, { forceRefresh: true })"
        >
          <svg
            class="h-3.5 w-3.5"
            :class="{ 'animate-spin': loading }"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M21 12a9 9 0 1 1-3-6.7" />
            <path d="M21 3v6h-6" />
          </svg>
        </button>

        <!-- 可点击的面包屑路径栏（如 / › root › 123，点任意一级直接跳转；点右侧空白处切换为手输路径） -->
        <form
          v-if="editingPath"
          class="min-w-0 flex-1"
          @submit.prevent="submitPathInput"
        >
          <input
            ref="pathInputEl"
            v-model="pathInput"
            type="text"
            class="w-full rounded-lg border border-zinc-400 bg-white px-2.5 py-1 font-mono text-xs text-zinc-900 outline-none"
            placeholder="输入完整路径回车跳转..."
            @blur="editingPath = false"
            @keydown.esc.stop="editingPath = false"
          />
        </form>
        <div
          v-else
          class="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto rounded-lg border border-zinc-200/80 bg-white px-2 py-1 font-mono text-xs text-zinc-700 cursor-text"
          title="点击路径分段直接跳转，点击空白处手动输入路径"
          @click.self="startEditPath"
        >
          <template v-for="(crumb, idx) in breadcrumbs" :key="crumb.path">
            <span v-if="idx > 0" class="px-0.5 text-zinc-300 select-none">/</span>
            <button
              type="button"
              class="shrink-0 rounded px-1 py-0.2 transition hover:bg-zinc-100 hover:text-zinc-950"
              :class="idx === breadcrumbs.length - 1 ? 'font-semibold text-zinc-900' : 'text-zinc-600'"
              @mouseenter="prefetchDir(crumb.path)"
              @click.stop="loadDir(crumb.path)"
            >
              {{ crumb.label }}
            </button>
          </template>
          <span class="min-w-[16px] flex-1 self-stretch" @click="startEditPath" />
        </div>

        <button
          v-if="currentPath !== 'drives'"
          type="button"
          class="inline-flex shrink-0 items-center gap-1 rounded-lg bg-zinc-900 px-2.5 py-1 text-xs font-medium text-white shadow-sm hover:bg-zinc-800"
          title="选择文件上传（或直接拖拽文件到此面板）"
          @click="triggerUploadPicker"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
          </svg>
          <span v-if="!compact">上传</span>
        </button>
      </div>

      <div v-if="currentPath !== 'drives'" class="flex items-center gap-1.5">
        <div class="relative min-w-0 flex-1">
          <input
            v-model="search"
            type="text"
            placeholder="过滤当前目录文件..."
            class="w-full rounded-lg border border-zinc-200/70 bg-white px-2.5 py-1 text-xs text-zinc-800 outline-none focus:border-zinc-400"
          />
        </div>
        <button
          type="button"
          class="rounded-lg border border-zinc-200/80 bg-white px-2 py-1 text-[11px] font-medium text-zinc-600 hover:bg-zinc-100"
          @click="openInlineAction('mkdir')"
        >
          +目录
        </button>
        <button
          type="button"
          class="rounded-lg border border-zinc-200/80 bg-white px-2 py-1 text-[11px] font-medium text-zinc-600 hover:bg-zinc-100"
          @click="openInlineAction('touch')"
        >
          +文件
        </button>
        <button
          type="button"
          class="rounded-lg border px-2 py-1 text-[11px] font-medium transition"
          :class="
            showHidden
              ? 'border-zinc-300 bg-zinc-200/70 text-zinc-900'
              : 'border-zinc-200/80 bg-white text-zinc-400'
          "
          @click="showHidden = !showHidden"
        >
          .*
        </button>
      </div>

      <!-- 行内新建 / 重命名 / 删除确认条 -->
      <form
        v-if="inlinePrompt"
        class="flex items-center gap-1.5 rounded-xl border border-zinc-300 bg-white p-2 shadow-sm"
        @submit.prevent="submitInlineAction"
      >
        <span class="shrink-0 text-xs font-medium text-zinc-700">{{ inlinePrompt.title }}</span>
        <input
          v-if="inlinePrompt.kind !== 'delete'"
          v-model="inlinePrompt.value"
          type="text"
          autofocus
          class="min-w-0 flex-1 rounded-lg border border-zinc-200 px-2 py-1 font-mono text-xs outline-none focus:border-zinc-400"
        />
        <div class="ml-auto flex shrink-0 items-center gap-1">
          <button
            type="submit"
            class="rounded-lg px-2.5 py-1 text-xs font-medium text-white"
            :class="inlinePrompt.kind === 'delete' ? 'bg-red-600 hover:bg-red-700' : 'bg-zinc-900 hover:bg-zinc-800'"
          >
            确认
          </button>
          <button
            type="button"
            class="rounded-lg px-2 py-1 text-xs text-zinc-500 hover:bg-zinc-100"
            @click="inlinePrompt = null"
          >
            取消
          </button>
        </div>
      </form>
    </div>

    <!-- 主体区：文件列表 + 内联编辑器 -->
    <div class="flex min-h-0 flex-1 divide-zinc-100 overflow-hidden" :class="!compact && editorOpen ? 'divide-x' : ''">
      <!-- 文件列表 -->
      <div
        ref="listContainerRef"
        v-show="!(compact && editorOpen)"
        class="flex min-h-0 flex-1 flex-col overflow-y-auto px-2 py-1.5"
      >
        <!-- 加载中骨架屏 -->
        <div v-if="loading && !items.length" class="space-y-1.5 p-2">
          <div
            v-for="n in 8"
            :key="n"
            class="flex items-center justify-between rounded-xl px-2.5 py-2"
          >
            <div class="flex items-center gap-2.5">
              <div class="h-4 w-4 animate-pulse rounded bg-zinc-200/80" />
              <div class="h-3.5 w-36 animate-pulse rounded bg-zinc-200/70" />
            </div>
            <div class="h-3 w-12 animate-pulse rounded bg-zinc-100" />
          </div>
        </div>

        <template v-else>
          <!-- 顶部第一行固定：.. / ... 返回上一级目录（包括从 /root 回到 /，从 C:/ 回到 此电脑盘符列表） -->
          <div
            v-if="parentPath"
            class="group flex items-center justify-between gap-2 rounded-xl px-2.5 py-1.5 text-xs font-medium text-zinc-600 transition cursor-pointer hover:bg-zinc-100/80 hover:text-zinc-900"
            @mouseenter="prefetchDir(parentPath)"
            @click="loadDir(parentPath)"
          >
            <div class="flex items-center gap-2">
              <svg
                class="h-4 w-4 shrink-0 text-zinc-500"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
              >
                <path d="M9 14l-4-4 4-4" />
                <path d="M5 10h11a4 4 0 1 1 0 8h-1" />
              </svg>
              <span class="font-mono font-semibold">..</span>
              <span class="text-[11px] text-zinc-400">
                {{ parentPath === 'drives' ? '（返回此电脑盘符列表）' : `（上级: ${parentPath}）` }}
              </span>
            </div>
          </div>

          <!-- 目录与文件条目 -->
          <div
            v-for="item in visibleItems"
            :key="item.path"
            :data-entry-path="item.path"
            class="group flex items-center justify-between gap-2 rounded-xl px-2.5 py-1.5 text-xs transition-all duration-300 cursor-pointer"
            :class="[
              highlightedPath === item.path
                ? 'bg-emerald-50/90 ring-1 ring-emerald-400/70 shadow-sm'
                : editorPath === item.path && editorOpen
                  ? 'bg-zinc-100 font-medium'
                  : 'hover:bg-zinc-100/80',
            ]"
            @mouseenter="onRowMouseEnter(item)"
            @mouseleave="onRowMouseLeave"
            @click="onEntryClick(item)"
          >
            <div class="flex min-w-0 items-center gap-2">
              <!-- 文件夹图标 -->
              <svg
                v-if="item.isDir"
                class="h-4 w-4 shrink-0 text-zinc-700"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
              </svg>
              <!-- 压缩包图标 -->
              <svg
                v-else-if="isArchiveFile(item)"
                class="h-4 w-4 shrink-0 text-amber-600"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="M21 8v13H3V8" />
                <path d="M1 3h22v5H1z" />
                <path d="M10 12h4" />
              </svg>
              <!-- 普通文件图标 -->
              <svg
                v-else
                class="h-4 w-4 shrink-0 text-zinc-400"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
              </svg>

              <span class="truncate text-zinc-800">{{ item.name }}</span>
              <span
                v-if="highlightedPath === item.path"
                class="shrink-0 rounded-md bg-emerald-500 px-1.5 py-0.2 text-[10px] font-medium text-white"
              >
                新生成
              </span>
              <span
                v-else-if="busyActionPath === item.path"
                class="shrink-0 rounded-md bg-zinc-900 px-1.5 py-0.2 text-[10px] font-medium text-white animate-pulse"
              >
                处理中...
              </span>
            </div>

            <!-- 右侧：文件大小/修改时间 与 悬停动作（下载 / 解压 / 压缩 / 改名 / 删除） -->
            <div v-if="currentPath !== 'drives'" class="flex shrink-0 items-center gap-2" @click.stop>
              <span v-if="!item.isDir" class="font-mono text-[11px] text-zinc-400 group-hover:hidden">
                {{ formatBytes(item.size) }}
              </span>
              <span v-if="!compact && item.mtime" class="hidden lg:inline font-mono text-[11px] text-zinc-300 group-hover:hidden">
                {{ formatTime(item.mtime) }}
              </span>

              <div class="hidden items-center gap-1 group-hover:flex">
                <button
                  v-if="isArchiveFile(item)"
                  type="button"
                  class="rounded-md bg-amber-500 px-1.5 py-0.5 text-[11px] font-medium text-white shadow-sm hover:bg-amber-600 disabled:opacity-50"
                  :disabled="busyActionPath === item.path"
                  title="解压到当前目录"
                  @click="handleExtract(item)"
                >
                  解压
                </button>
                <button
                  v-if="!item.isDir"
                  type="button"
                  class="rounded-md bg-white px-1.5 py-0.5 text-[11px] font-medium text-zinc-700 shadow-sm ring-1 ring-zinc-200 hover:bg-zinc-900 hover:text-white"
                  title="满速流式下载"
                  @click="triggerDownload(item)"
                >
                  下载
                </button>
                <button
                  type="button"
                  class="rounded-md bg-white px-1.5 py-0.5 text-[11px] text-zinc-600 shadow-sm ring-1 ring-zinc-200 hover:text-zinc-900 disabled:opacity-50"
                  :disabled="busyActionPath === item.path"
                  title="打包压缩"
                  @click="handleCompress(item)"
                >
                  压缩
                </button>
                <button
                  type="button"
                  class="rounded-md bg-white px-1.5 py-0.5 text-[11px] text-zinc-600 shadow-sm ring-1 ring-zinc-200 hover:text-zinc-900"
                  title="复制完整路径"
                  @click="copyEntryPath(item.path)"
                >
                  路径
                </button>
                <button
                  type="button"
                  class="rounded-md bg-white px-1.5 py-0.5 text-[11px] text-zinc-600 shadow-sm ring-1 ring-zinc-200 hover:text-zinc-900"
                  title="重命名"
                  @click="openInlineAction('rename', item)"
                >
                  改名
                </button>
                <button
                  type="button"
                  class="rounded-md bg-white px-1.5 py-0.5 text-[11px] text-red-600 shadow-sm ring-1 ring-zinc-200 hover:bg-red-50"
                  title="删除"
                  @click="openInlineAction('delete', item)"
                >
                  删除
                </button>
              </div>
            </div>
          </div>

          <div
            v-if="!loading && !visibleItems.length"
            class="flex flex-1 items-center justify-center py-10 text-xs text-zinc-400"
          >
            当前目录为空
          </div>
        </template>
      </div>

      <!-- 在线文本编辑器面板 -->
      <div
        v-if="editorOpen"
        class="flex min-h-0 flex-1 flex-col overflow-hidden bg-white"
      >
        <div class="flex items-center justify-between border-b border-zinc-100 bg-zinc-50/70 px-3 py-2">
          <div class="min-w-0">
            <p class="truncate font-mono text-xs font-semibold text-zinc-800">{{ editorName }}</p>
            <p class="truncate font-mono text-[10px] text-zinc-400">{{ editorPath }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              class="rounded-lg bg-zinc-900 px-3 py-1 text-xs font-medium text-white hover:bg-zinc-800 disabled:opacity-50"
              :disabled="editorSaving || editorLoading"
              @click="saveFileEditor"
            >
              {{ editorSaving ? '保存中...' : '保存 (Ctrl+S)' }}
            </button>
            <button
              type="button"
              class="rounded-lg px-2 py-1 text-xs text-zinc-500 hover:bg-zinc-100"
              @click="editorOpen = false"
            >
              关闭
            </button>
          </div>
        </div>
        <div v-if="editorLoading" class="flex flex-1 items-center justify-center text-xs text-zinc-400">
          正在读取文件内容...
        </div>
        <CodeEditor
          v-else
          v-model="editorContent"
          :filename="editorName"
          @save="saveFileEditor"
        />
      </div>
    </div>

    <!-- 底部：高速传输队列面板 -->
    <div
      v-if="transfers.length"
      class="shrink-0 border-t border-zinc-200/80 bg-zinc-50/90 px-3 py-2"
    >
      <div class="mb-1.5 flex items-center justify-between text-[11px]">
        <span class="font-semibold text-zinc-600">
          传输队列 ({{ transfers.length }})
        </span>
        <button
          type="button"
          class="text-zinc-400 hover:text-zinc-700"
          @click="clearCompletedTransfers"
        >
          清理已完成
        </button>
      </div>
      <div class="max-h-32 space-y-1.5 overflow-y-auto">
        <div
          v-for="t in transfers.slice(0, 6)"
          :key="t.id"
          class="rounded-xl border border-zinc-200/80 bg-white px-2.5 py-1.5 text-[11px]"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="truncate font-medium text-zinc-800">
              {{ t.direction === 'upload' ? '⬆' : '⬇' }} {{ t.filename }}
              <span v-if="t.resumedFrom > 0" class="ml-1 font-normal text-zinc-400">
                续传自 {{ formatBytes(t.resumedFrom) }}
              </span>
            </span>
            <span class="shrink-0 font-mono text-[10px] text-zinc-500">
              <template v-if="t.state === 'running'">
                {{ formatSpeed(t.speedBps) }} · {{ formatBytes(t.transferredBytes) }} / {{ formatBytes(t.totalBytes) }}
              </template>
              <template v-else-if="t.state === 'completed'">
                <span class="text-emerald-600">
                  ✓ 完成 ({{ formatSpeed(t.speedBps) }}){{ t.verified ? ' 已校验' : '' }}
                </span>
              </template>
              <template v-else>
                <span class="text-red-600" :title="t.error || ''">✗ 失败</span>
              </template>
            </span>
          </div>
          <div class="mt-1 h-1 w-full overflow-hidden rounded-full bg-zinc-100">
            <div
              class="h-full rounded-full transition-all duration-200"
              :class="
                t.state === 'failed'
                  ? 'bg-red-500'
                  : t.state === 'completed'
                    ? 'bg-emerald-500'
                    : 'bg-zinc-900'
              "
              :style="{
                width:
                  t.totalBytes > 0
                    ? `${Math.min(100, Math.round((t.transferredBytes * 100) / t.totalBytes))}%`
                    : t.state === 'completed'
                      ? '100%'
                      : '30%',
              }"
            />
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
