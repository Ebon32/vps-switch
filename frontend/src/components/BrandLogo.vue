<script setup lang="ts">
defineProps<{
  size?: 'sm' | 'md' | 'lg';
}>();
</script>

<template>
  <div
    class=" relative inline-flex shrink-0 items-center justify-center overflow-hidden bg-zinc-900 shadow-[0_2px_8px_rgba(24,24,27,0.18)] ring-1 ring-black/5 dark:bg-zinc-100 dark:ring-white/10"
    :class="{
      'h-7 w-7 rounded-lg': !size || size === 'sm',
      'h-9 w-9 rounded-xl': size === 'md',
      'h-12 w-12 rounded-2xl': size === 'lg',
    }"
  >
    <svg
      class="h-[72%] w-[72%]"
      viewBox="0 0 40 40"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <!-- 左侧终端命令符 > -->
      <path
        d="M8.5 12.5L16.5 20L8.5 27.5"
        class="stroke-white dark:stroke-zinc-900"
        stroke-width="3.4"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <!-- 右侧 Switch 胶囊拨杆轨道 -->
      <rect
        x="19.5"
        y="14.5"
        width="13"
        height="11"
        rx="5.5"
        class="stroke-white/35 fill-white/10 dark:stroke-zinc-900/35 dark:fill-zinc-900/10"
        stroke-width="2"
      />
      <!-- 激活态活力橙圆点（呼应右上角 + 与 Switch 概念） -->
      <circle cx="27" cy="20" r="3.4" fill="#F97316" />
    </svg>
  </div>
</template>
