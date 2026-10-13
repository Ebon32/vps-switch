<script setup lang="ts">
defineProps<{
  size?: 'sm' | 'md' | 'lg';
}>();
</script>

<template>
  <div
    class="relative inline-flex shrink-0 items-center justify-center overflow-hidden bg-black shadow-[0_2px_8px_rgba(0,0,0,0.3)] ring-1 ring-white/15 dark:bg-black dark:ring-white/20"
    :class="{
      'h-7 w-7 rounded-lg': !size || size === 'sm',
      'h-9 w-9 rounded-xl': size === 'md',
      'h-12 w-12 rounded-2xl': size === 'lg',
    }"
  >
    <svg
      class="h-[75%] w-[75%]"
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path d="M16 18 H38 V26 H24 V40 H16 Z" fill="#FFFFFF" />
      <path d="M48 46 H26 V38 H40 V24 H48 Z" fill="#FFFFFF" />
      <circle cx="32" cy="32" r="3.5" fill="#FFFFFF" />
    </svg>
  </div>
</template>
