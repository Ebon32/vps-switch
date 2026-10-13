<script setup lang="ts">
import { computed } from 'vue';
import type { OsInfo } from '../types';

const props = defineProps<{
  osInfo?: OsInfo | null;
  kind?: 'local' | 'ssh';
  name?: string;
  size?: 'sm' | 'md';
}>();

const distro = computed(() => {
  const id = (props.osInfo?.distroId || '').toLowerCase();
  const pretty = (props.osInfo?.prettyName || '').toLowerCase();
  const os = (props.osInfo?.os || '').toLowerCase();
  if (os.includes('win') || id.includes('win') || pretty.includes('windows')) return 'windows';
  if (id.includes('ubuntu') || pretty.includes('ubuntu')) return 'ubuntu';
  if (id.includes('debian') || pretty.includes('debian')) return 'debian';
  if (id.includes('alpine') || pretty.includes('alpine')) return 'alpine';
  return 'linux';
});

const label = computed(() => {
  switch (distro.value) {
    case 'ubuntu':
      return 'UB';
    case 'debian':
      return 'DB';
    case 'alpine':
      return 'AL';
    case 'windows':
      return 'WN';
    default:
      return (props.name || 'VP').slice(0, 2).toUpperCase();
  }
});
</script>

<template>
  <div
    class="inline-flex shrink-0 items-center justify-center rounded-full border border-zinc-200/80 bg-zinc-100/90 font-semibold tracking-tight text-zinc-700 select-none dark:border-zinc-700/80 dark:bg-zinc-800/90 dark:text-zinc-200"
    :class="size === 'sm' ? 'h-6 w-6 text-[10px]' : 'h-10 w-10 text-xs'"
  >
    <!-- Ubuntu -->
    <svg v-if="distro === 'ubuntu'" class="h-4 w-4 text-orange-600" viewBox="0 0 24 24" fill="currentColor">
      <circle cx="12" cy="12" r="4.2" fill="none" stroke="currentColor" stroke-width="2" />
      <circle cx="19" cy="12" r="1.8" />
      <circle cx="8.5" cy="5.8" r="1.8" />
      <circle cx="8.5" cy="18.2" r="1.8" />
    </svg>
    <!-- Debian -->
    <svg v-else-if="distro === 'debian'" class="h-4 w-4 text-rose-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
      <path d="M12 15a3 3 0 1 0-3-3c0 3 2.5 5.5 6 5.5 4 0 6-3 6-6.5C21 6.5 17 3 12 3S3 6.8 3 12c0 5 4 9 9 9" />
    </svg>
    <!-- Alpine -->
    <svg v-else-if="distro === 'alpine'" class="h-4 w-4 text-zinc-700 dark:text-zinc-300" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round">
      <path d="M3 18L9.5 7l4.5 7.5L16.5 11 21 18H3z" />
    </svg>
    <!-- Windows -->
    <svg v-else-if="distro === 'windows'" class="h-3.5 w-3.5 text-zinc-700 dark:text-zinc-300" viewBox="0 0 24 24" fill="currentColor">
      <path d="M3 3h8.5v8.5H3V3zm9.5 0H21v8.5h-8.5V3zM3 12.5h8.5V21H3v-8.5zm9.5 0H21V21h-8.5v-8.5z" />
    </svg>
    <span v-else>{{ label }}</span>
  </div>
</template>
