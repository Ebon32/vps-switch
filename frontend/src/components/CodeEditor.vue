<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { Compartment, EditorState } from '@codemirror/state';
import { keymap } from '@codemirror/view';
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';
import { json } from '@codemirror/lang-json';
import { yaml } from '@codemirror/lang-yaml';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { nginx } from '@codemirror/legacy-modes/mode/nginx';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { dockerFile } from '@codemirror/legacy-modes/mode/dockerfile';
import { standardSQL } from '@codemirror/legacy-modes/mode/sql';
import { powerShell } from '@codemirror/legacy-modes/mode/powershell';
import { xml } from '@codemirror/legacy-modes/mode/xml';
import { diff } from '@codemirror/legacy-modes/mode/diff';
import { lua } from '@codemirror/legacy-modes/mode/lua';

const props = defineProps<{
  modelValue: string;
  filename?: string;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string];
  save: [];
}>();

const hostEl = ref<HTMLDivElement | null>(null);
let view: EditorView | null = null;
const language = new Compartment();

/// 配色全部走 CSS 变量，由根节点的 .dark 类切换，不需要在 JS 里再维护一套深色主题
const chromeTheme = EditorView.theme({
  '&': {
    height: '100%',
    fontSize: '12px',
    backgroundColor: 'var(--cm-bg)',
    color: 'var(--cm-fg)',
  },
  '.cm-scroller': {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
    lineHeight: '1.65',
  },
  '.cm-content': { caretColor: 'var(--cm-fg)' },
  '.cm-gutters': {
    backgroundColor: 'var(--cm-gutter-bg)',
    color: 'var(--cm-gutter-fg)',
    border: 'none',
  },
  '.cm-activeLine': { backgroundColor: 'var(--cm-active-line)' },
  '.cm-activeLineGutter': {
    backgroundColor: 'var(--cm-active-line)',
    color: 'var(--cm-fg)',
  },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': {
    backgroundColor: 'var(--cm-selection)',
  },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--cm-fg)' },
  '.cm-panels': {
    backgroundColor: 'var(--cm-gutter-bg)',
    color: 'var(--cm-fg)',
    border: 'none',
  },
  '.cm-panel input, .cm-panel button': {
    backgroundColor: 'var(--cm-bg)',
    color: 'var(--cm-fg)',
    border: '1px solid var(--cm-gutter-fg)',
    borderRadius: '6px',
    padding: '1px 5px',
  },
  '.cm-searchMatch': { backgroundColor: 'var(--cm-search-match)' },
  '.cm-searchMatch.cm-searchMatch-selected': {
    backgroundColor: 'var(--cm-search-active)',
  },
  '.cm-selectionMatch': { backgroundColor: 'var(--cm-search-match)' },
});

const highlight = HighlightStyle.define([
  { tag: t.comment, color: 'var(--cm-comment)', fontStyle: 'italic' },
  { tag: [t.keyword, t.modifier, t.controlKeyword], color: 'var(--cm-keyword)' },
  { tag: [t.string, t.special(t.string), t.regexp], color: 'var(--cm-string)' },
  { tag: [t.number, t.bool, t.null, t.atom], color: 'var(--cm-number)' },
  { tag: [t.propertyName, t.definition(t.variableName)], color: 'var(--cm-property)' },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: 'var(--cm-function)' },
  { tag: [t.typeName, t.className, t.tagName], color: 'var(--cm-type)' },
  { tag: [t.operator, t.punctuation, t.bracket], color: 'var(--cm-punct)' },
  { tag: t.attributeName, color: 'var(--cm-property)' },
  { tag: t.heading, color: 'var(--cm-keyword)', fontWeight: 'bold' },
  { tag: t.link, color: 'var(--cm-string)', textDecoration: 'underline' },
  { tag: t.invalid, color: '#ef4444' },
]);

/// 按文件名挑语法：VPS 上最常改的就是 conf / yaml / json / sh / ini
function languageFor(filename: string) {
  const name = (filename || '').toLowerCase();
  const base = name.split('/').pop() || name;
  const ext = base.includes('.') ? base.slice(base.lastIndexOf('.') + 1) : '';

  if (base === 'dockerfile' || base.startsWith('dockerfile.')) {
    return StreamLanguage.define(dockerFile);
  }
  if (/^(\.bashrc|\.bash_profile|\.profile|\.zshrc|\.env)$/.test(base)) {
    return StreamLanguage.define(shell);
  }
  if (base === 'nginx.conf' || base === 'sshd_config' || base === 'ssh_config') {
    return StreamLanguage.define(nginx);
  }

  switch (ext) {
    case 'json':
    case 'jsonc':
    case 'json5':
      return json();
    case 'yaml':
    case 'yml':
      return yaml();
    case 'js':
    case 'mjs':
    case 'cjs':
      return javascript();
    case 'ts':
    case 'mts':
    case 'cts':
      return javascript({ typescript: true });
    case 'jsx':
      return javascript({ jsx: true });
    case 'tsx':
      return javascript({ typescript: true, jsx: true });
    case 'py':
    case 'pyw':
      return python();
    case 'sh':
    case 'bash':
    case 'zsh':
    case 'ksh':
      return StreamLanguage.define(shell);
    case 'ps1':
    case 'psm1':
      return StreamLanguage.define(powerShell);
    case 'conf':
    case 'cnf':
    case 'vhost':
      return StreamLanguage.define(nginx);
    case 'ini':
    case 'cfg':
    case 'properties':
    case 'env':
    case 'service':
      return StreamLanguage.define(properties);
    case 'toml':
      return StreamLanguage.define(toml);
    case 'sql':
      return StreamLanguage.define(standardSQL);
    case 'xml':
    case 'html':
    case 'htm':
    case 'vue':
    case 'svg':
      return StreamLanguage.define(xml);
    case 'diff':
    case 'patch':
      return StreamLanguage.define(diff);
    case 'lua':
      return StreamLanguage.define(lua);
    default:
      return [];
  }
}

onMounted(() => {
  if (!hostEl.value) return;
  view = new EditorView({
    parent: hostEl.value,
    state: EditorState.create({
      doc: props.modelValue,
      extensions: [
        basicSetup,
        syntaxHighlighting(highlight),
        chromeTheme,
        EditorView.lineWrapping,
        language.of(languageFor(props.filename || '')),
        keymap.of([
          {
            key: 'Mod-s',
            preventDefault: true,
            run: () => {
              emit('save');
              return true;
            },
          },
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            emit('update:modelValue', u.state.doc.toString());
          }
        }),
      ],
    }),
  });
  view.focus();
});

// 外部换了文件（或读取完成后填进内容）：整篇替换，不走增量事件
watch(
  () => props.modelValue,
  (next) => {
    if (!view) return;
    const current = view.state.doc.toString();
    if (next === current) return;
    view.dispatch({
      changes: { from: 0, to: current.length, insert: next },
    });
  }
);

watch(
  () => props.filename,
  (next) => {
    if (!view) return;
    view.dispatch({
      effects: language.reconfigure(languageFor(next || '')),
    });
  }
);

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});
</script>

<template>
  <div ref="hostEl" class="cm-host min-h-0 flex-1 overflow-hidden" />
</template>

<style>
:root {
  --cm-bg: #ffffff;
  --cm-fg: #27272a;
  --cm-gutter-bg: #fafafa;
  --cm-gutter-fg: #a1a1aa;
  --cm-active-line: rgba(161, 161, 170, 0.1);
  --cm-selection: rgba(39, 39, 42, 0.16);
  --cm-search-match: rgba(250, 204, 21, 0.35);
  --cm-search-active: rgba(250, 204, 21, 0.65);
  --cm-comment: #94a3b8;
  --cm-keyword: #7c3aed;
  --cm-string: #047857;
  --cm-number: #b45309;
  --cm-property: #1d4ed8;
  --cm-function: #0e7490;
  --cm-type: #be185d;
  --cm-punct: #71717a;
}

.dark {
  --cm-bg: #09090b;
  --cm-fg: #e4e4e7;
  --cm-gutter-bg: #111113;
  --cm-gutter-fg: #52525b;
  --cm-active-line: rgba(161, 161, 170, 0.12);
  --cm-selection: rgba(228, 228, 231, 0.2);
  --cm-search-match: rgba(250, 204, 21, 0.25);
  --cm-search-active: rgba(250, 204, 21, 0.5);
  --cm-comment: #6b7280;
  --cm-keyword: #c4b5fd;
  --cm-string: #6ee7b7;
  --cm-number: #fcd34d;
  --cm-property: #93c5fd;
  --cm-function: #67e8f9;
  --cm-type: #f9a8d4;
  --cm-punct: #a1a1aa;
}

.cm-host .cm-editor {
  height: 100%;
}

.cm-host .cm-editor.cm-focused {
  outline: none;
}
</style>
