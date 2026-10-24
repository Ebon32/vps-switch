//! 板块三（文件浏览器）+ 在线代码编辑器 + 传输队列面板
//! （1:1 对齐 `FileBrowserPanel.vue` + `CodeEditor.vue`）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::{ApiCommand, FileEntry, Host, TransferTask};
use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::util::{format_bytes, format_mtime, format_rate};
use crate::widgets::lucide_icon;
use crate::workspace::{RightWing, ToastKind, Workspace};

const ARCHIVE_SUFFIXES: &[&str] = &[
    ".tar.gz",
    ".tgz",
    ".tar.xz",
    ".txz",
    ".tar.bz2",
    ".tbz2",
    ".tar",
    ".zip",
    ".gz",
];

pub fn is_archive_file(item: &FileEntry) -> bool {
    if item.is_dir {
        return false;
    }
    let lower = item.name.to_lowercase();
    ARCHIVE_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

#[derive(Clone, Debug)]
pub struct DirCacheItem {
    pub path: String,
    pub parent: Option<String>,
    pub items: Vec<FileEntry>,
    pub at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Mkdir,
    Touch,
    Rename,
    Delete,
    Download,
}

#[derive(Clone, Debug)]
pub struct InlinePrompt {
    pub kind: PromptKind,
    pub title: String,
    pub target: Option<FileEntry>,
}

#[derive(Clone, Debug)]
pub struct Breadcrumb {
    pub label: String,
    pub path: String,
}

pub struct FileBrowserState {
    pub host_id: Option<String>,
    pub current_path: String,
    pub parent_path: Option<String>,
    pub items: Vec<FileEntry>,
    pub loading: bool,
    pub dir_cache: HashMap<String, DirCacheItem>,

    pub editing_path: bool,
    pub path_input: Entity<InputState>,
    pub search_input: Entity<InputState>,
    pub search_query: String,
    pub show_hidden: bool,

    pub inline_prompt: Option<InlinePrompt>,
    pub prompt_input: Entity<InputState>,

    pub busy_action_path: Option<String>,
    pub highlighted: Option<(String, Instant)>,

    pub editor_open: bool,
    pub editor_loading: bool,
    pub editor_saving: bool,
    pub editor_path: String,
    pub editor_name: String,
    pub editor_state: Entity<EditorState>,
    pub pending_editor_load: Option<(String, String)>,

    pub transfers: Vec<TransferTask>,
    pub last_transfer_poll: Option<Instant>,
}

impl FileBrowserState {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let path_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("输入完整路径回车跳转...")
        });
        cx.subscribe_in(
            &path_input,
            window,
            |ws, _, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.submit_file_path_input(cx);
                }
            },
        )
        .detach();

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("过滤当前目录文件..."));
        cx.observe(&search_input, |ws, state, cx| {
            ws.files.search_query = state.read(cx).value().to_string();
            cx.notify();
        })
        .detach();

        let prompt_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(
            &prompt_input,
            window,
            |ws, _, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.submit_file_inline_prompt(cx);
                }
            },
        )
        .detach();

        let editor_state = cx.new(|cx| EditorState::new(window, cx).language("plaintext"));

        Self {
            host_id: None,
            current_path: String::new(),
            parent_path: None,
            items: Vec::new(),
            loading: false,
            dir_cache: HashMap::new(),
            editing_path: false,
            path_input,
            search_input,
            search_query: String::new(),
            show_hidden: true,
            inline_prompt: None,
            prompt_input,
            busy_action_path: None,
            highlighted: None,
            editor_open: false,
            editor_loading: false,
            editor_saving: false,
            editor_path: String::new(),
            editor_name: String::new(),
            editor_state,
            pending_editor_load: None,
            transfers: Vec::new(),
            last_transfer_poll: None,
        }
    }

    pub fn cache_key(host_id: &str, path: &str) -> String {
        let norm = if path.trim().is_empty() {
            "~"
        } else {
            path.trim()
        };
        format!("{host_id}::{norm}")
    }

    pub fn invalidate_cache(&mut self, host_id: &str, path: &str) {
        self.dir_cache.remove(&Self::cache_key(host_id, path));
    }

    pub fn store_cache(
        &mut self,
        host_id: &str,
        requested_path: &str,
        path: String,
        parent: Option<String>,
        items: Vec<FileEntry>,
    ) {
        let entry = DirCacheItem {
            path: path.clone(),
            parent,
            items,
            at: Instant::now(),
        };
        self.dir_cache
            .insert(Self::cache_key(host_id, &path), entry.clone());
        if requested_path != path {
            self.dir_cache
                .insert(Self::cache_key(host_id, requested_path), entry);
        }
    }

    pub fn visible_items(&self) -> Vec<&FileEntry> {
        let q = self.search_query.trim().to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                if !self.show_hidden && item.name.starts_with('.') {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                item.name.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn breadcrumbs(&self, host: &Host) -> Vec<Breadcrumb> {
        let p = self.current_path.trim();
        let is_local_win =
            host.kind == "local" && host.os.as_deref().unwrap_or("").eq_ignore_ascii_case("windows");
        if is_local_win {
            let mut crumbs = vec![Breadcrumb {
                label: "此电脑".into(),
                path: "drives".into(),
            }];
            if p.is_empty() || p == "drives" {
                return crumbs;
            }
            let norm = p.replace('\\', "/");
            let norm = norm.trim_end_matches('/');
            let parts: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
            let mut acc = String::new();
            for (i, seg) in parts.into_iter().enumerate() {
                if i == 0 && seg.ends_with(':') {
                    acc = format!("{seg}/");
                    crumbs.push(Breadcrumb {
                        label: seg.to_owned(),
                        path: acc.clone(),
                    });
                } else {
                    if acc.ends_with('/') {
                        acc.push_str(seg);
                    } else {
                        acc = format!("{acc}/{seg}");
                    }
                    crumbs.push(Breadcrumb {
                        label: seg.to_owned(),
                        path: acc.clone(),
                    });
                }
            }
            return crumbs;
        }

        let mut crumbs = vec![Breadcrumb {
            label: "/".into(),
            path: "/".into(),
        }];
        if p.is_empty() || p == "/" {
            return crumbs;
        }
        let mut acc = String::new();
        for seg in p.split('/').filter(|s| !s.is_empty()) {
            acc = format!("{acc}/{seg}");
            crumbs.push(Breadcrumb {
                label: seg.to_owned(),
                path: acc.clone(),
            });
        }
        crumbs
    }

    pub fn highlighted_path(&self) -> Option<&str> {
        self.highlighted
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(4))
            .map(|(p, _)| p.as_str())
    }
}

pub fn join_remote_path(dir: &str, name: &str) -> String {
    let clean = dir.trim_end_matches('/');
    if clean.is_empty() || clean == "/" {
        format!("/{name}")
    } else {
        format!("{clean}/{name}")
    }
}

pub fn compute_optimistic_parent(p: &str) -> Option<String> {
    if p.is_empty() || p == "/" || p == "drives" {
        return None;
    }
    if (p.len() == 2 && p.ends_with(':')) || (p.len() == 3 && p.ends_with(":/")) {
        return Some("drives".into());
    }
    let idx = p.rfind('/')?;
    if idx == 0 {
        return Some("/".into());
    }
    let sub = &p[..idx];
    if sub.len() == 2 && sub.ends_with(':') {
        Some(format!("{sub}/"))
    } else {
        Some(sub.to_owned())
    }
}

pub fn editor_language_for(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base == "dockerfile" || base.starts_with("dockerfile.") {
        return "bash";
    }
    if matches!(
        base,
        ".bashrc" | ".bash_profile" | ".profile" | ".zshrc" | ".env"
    ) {
        return "bash";
    }
    let ext = base.rsplit('.').next().unwrap_or("");
    match ext {
        "json" | "jsonc" | "json5" => "json",
        "yaml" | "yml" => "yaml",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" | "tsx" => "typescript",
        "py" | "pyw" => "python",
        "sh" | "bash" | "zsh" | "ksh" => "bash",
        "toml" => "toml",
        "sql" => "sql",
        "html" | "htm" | "vue" | "svg" | "xml" => "html",
        "css" => "css",
        "md" | "markdown" => "markdown",
        "rs" => "rust",
        "go" => "go",
        _ => "plaintext",
    }
}

impl Workspace {
    /// 确保文件浏览器已切换到 `host_id`，必要时发起目录加载。
    pub fn ensure_files_host(&mut self, host_id: &str, initial_path: Option<&str>) {
        if self.files.host_id.as_deref() == Some(host_id)
            && (!self.files.items.is_empty() || self.files.loading || !self.files.current_path.is_empty())
        {
            return;
        }
        self.files.host_id = Some(host_id.to_owned());
        self.files.editor_open = false;
        self.files.inline_prompt = None;
        self.files.current_path.clear();
        self.files.parent_path = None;
        self.files.items.clear();
        let target = initial_path.unwrap_or("").to_owned();
        self.load_files_dir(&target, false);
        self.api.send(ApiCommand::ListTransfers {
            host_id: host_id.to_owned(),
        });
    }

    pub fn load_files_dir(&mut self, target_path: &str, force_refresh: bool) {
        let Some(host_id) = self.files.host_id.clone() else {
            return;
        };
        let req_path = target_path.trim().to_owned();
        let is_same_dir = req_path == self.files.current_path && !self.files.items.is_empty();

        self.files.editing_path = false;
        self.files.inline_prompt = None;
        if !is_same_dir {
            self.files.search_query.clear();
        }

        if !req_path.is_empty() && !is_same_dir {
            self.files.current_path = req_path.clone();
            self.files.parent_path = compute_optimistic_parent(&req_path);
        }

        if !force_refresh
            && let Some(cached) = self
                .files
                .dir_cache
                .get(&FileBrowserState::cache_key(&host_id, &req_path))
                .cloned()
        {
            self.files.current_path = cached.path;
            self.files.parent_path = cached.parent.clone();
            self.files.items = cached.items;
            self.files.loading = false;
            if cached.at.elapsed() < Duration::from_secs(15) {
                if let Some(parent) = cached.parent {
                    self.prefetch_files_dir(&host_id, &parent);
                }
                return;
            }
        } else if !is_same_dir {
            self.files.items.clear();
            self.files.loading = true;
        } else {
            self.files.loading = true;
        }

        self.api.send(ApiCommand::ListFiles {
            host_id,
            path: req_path,
            force_refresh,
            prefetch_only: false,
        });
    }

    pub fn prefetch_files_dir(&self, host_id: &str, path: &str) {
        if path.is_empty() {
            return;
        }
        let key = FileBrowserState::cache_key(host_id, path);
        if let Some(cached) = self.files.dir_cache.get(&key)
            && cached.at.elapsed() < Duration::from_secs(30)
        {
            return;
        }
        self.api.send(ApiCommand::ListFiles {
            host_id: host_id.to_owned(),
            path: path.to_owned(),
            force_refresh: false,
            prefetch_only: true,
        });
    }

    pub fn start_edit_file_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let val = if self.files.current_path == "drives" {
            String::new()
        } else {
            self.files.current_path.clone()
        };
        self.files.editing_path = true;
        self.files.path_input.update(cx, |input, cx| {
            input.set_value(val, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub fn submit_file_path_input(&mut self, cx: &mut Context<Self>) {
        self.files.editing_path = false;
        let target = self.files.path_input.read(cx).value().trim().to_owned();
        if !target.is_empty() && target != self.files.current_path {
            self.load_files_dir(&target, false);
        }
        cx.notify();
    }

    pub fn open_file_inline_prompt(
        &mut self,
        kind: PromptKind,
        target: Option<FileEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, initial_val) = match &kind {
            PromptKind::Mkdir => ("新建文件夹名称".to_owned(), String::new()),
            PromptKind::Touch => ("新建文件名称".to_owned(), String::new()),
            PromptKind::Rename => {
                let name = target.as_ref().map(|t| t.name.clone()).unwrap_or_default();
                (format!("重命名 {name}"), name)
            }
            PromptKind::Delete => {
                let (kind_str, name) = target
                    .as_ref()
                    .map(|t| (if t.is_dir { "目录" } else { "文件" }, t.name.as_str()))
                    .unwrap_or(("文件", ""));
                (format!("确认删除{kind_str}「{name}」？"), String::new())
            }
            PromptKind::Download => {
                let name = target.as_ref().map(|t| t.name.as_str()).unwrap_or("");
                (
                    format!("下载「{name}」到本机路径（留空 = 系统下载目录）"),
                    String::new(),
                )
            }
        };
        if kind != PromptKind::Delete {
            self.files.prompt_input.update(cx, |input, cx| {
                input.set_value(initial_val, window, cx);
                input.focus(window, cx);
            });
        }
        self.files.inline_prompt = Some(InlinePrompt {
            kind,
            title,
            target,
        });
        cx.notify();
    }

    pub fn submit_file_inline_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.files.inline_prompt.take() else {
            return;
        };
        let Some(host_id) = self.files.host_id.clone() else {
            return;
        };
        let val = self.files.prompt_input.read(cx).value().trim().to_owned();
        let cur_dir = self.files.current_path.clone();

        match prompt.kind {
            PromptKind::Mkdir => {
                if val.is_empty() {
                    return;
                }
                let target_path = join_remote_path(&cur_dir, &val);
                self.api.send(ApiCommand::Mkdir {
                    host_id,
                    dir_to_reload: cur_dir,
                    target_path,
                    folder_name: val,
                });
            }
            PromptKind::Touch => {
                if val.is_empty() {
                    return;
                }
                let target_path = join_remote_path(&cur_dir, &val);
                self.api.send(ApiCommand::WriteFile {
                    host_id,
                    path: target_path,
                    name: val,
                    content: String::new(),
                    from_editor: false,
                    dir_to_reload: cur_dir,
                });
            }
            PromptKind::Rename => {
                let Some(target) = prompt.target else {
                    return;
                };
                if val.is_empty() || val == target.name {
                    cx.notify();
                    return;
                }
                let new_path = join_remote_path(&cur_dir, &val);
                self.api.send(ApiCommand::RenameFile {
                    host_id,
                    dir_to_reload: cur_dir,
                    old_path: target.path,
                    new_path,
                    new_name: val,
                });
            }
            PromptKind::Delete => {
                let Some(target) = prompt.target else {
                    return;
                };
                if self.files.editor_path == target.path {
                    self.files.editor_open = false;
                }
                self.api.send(ApiCommand::RemoveFile {
                    host_id,
                    dir_to_reload: cur_dir,
                    path: target.path,
                    name: target.name,
                    is_dir: target.is_dir,
                });
            }
            PromptKind::Download => {
                let Some(target) = prompt.target else {
                    return;
                };
                self.api.send(ApiCommand::StartTransfer {
                    host_id,
                    direction: "download".into(),
                    local_path: val,
                    remote_path: target.path,
                    dir_to_reload: None,
                });
            }
        }
        cx.notify();
    }

    pub fn open_file_in_editor(&mut self, item: &FileEntry, cx: &mut Context<Self>) {
        let Some(host_id) = self.files.host_id.clone() else {
            return;
        };
        self.files.editor_open = true;
        self.files.editor_loading = true;
        self.files.editor_path = item.path.clone();
        self.files.editor_name = item.name.clone();
        self.api.send(ApiCommand::ReadFile {
            host_id,
            path: item.path.clone(),
            name: item.name.clone(),
        });
        cx.notify();
    }

    pub fn save_editor_file(&mut self, cx: &mut Context<Self>) {
        if !self.files.editor_open
            || self.files.editor_loading
            || self.files.editor_saving
            || self.files.editor_path.is_empty()
        {
            return;
        }
        let Some(host_id) = self.files.host_id.clone() else {
            return;
        };
        let content = self.files.editor_state.read(cx).value().to_string();
        self.files.editor_saving = true;
        self.api.send(ApiCommand::WriteFile {
            host_id,
            path: self.files.editor_path.clone(),
            name: self.files.editor_name.clone(),
            content,
            from_editor: true,
            dir_to_reload: self.files.current_path.clone(),
        });
        cx.notify();
    }

    pub fn trigger_native_upload_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(host_id) = self.files.host_id.clone() else {
            return;
        };
        let cur_dir = self.files.current_path.clone();
        if cur_dir.is_empty() || cur_dir == "drives" {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    ws.upload_local_paths(&host_id, &cur_dir, &paths, cx);
                });
            }
        })
        .detach();
    }

    pub fn upload_local_paths(
        &mut self,
        host_id: &str,
        dir_path: &str,
        paths: &[std::path::PathBuf],
        cx: &mut Context<Self>,
    ) {
        if dir_path.is_empty() || dir_path == "drives" {
            return;
        }
        for path in paths {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let remote_path = join_remote_path(dir_path, name);
            self.api.send(ApiCommand::StartTransfer {
                host_id: host_id.to_owned(),
                direction: "upload".into(),
                local_path: path.to_string_lossy().to_string(),
                remote_path,
                dir_to_reload: Some(dir_path.to_owned()),
            });
        }
        self.files.last_transfer_poll = None;
        cx.notify();
    }
}

/// 渲染文件浏览器面板（`compact = false` 为中央全宽舞台，`compact = true` 为右侧 336px 栏）。
pub fn render(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    compact: bool,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let Some(host_id) = workspace.files.host_id.as_deref() else {
        return div().into_any_element();
    };
    let Some(host) = workspace.host_by_id(host_id) else {
        return div().into_any_element();
    };

    let files = &workspace.files;
    let active_cwd = workspace.active_terminal_cwd(host_id, cx);

    // ---- 1. 顶部栏 ----
    let title = if compact {
        format!("{} · 文件", host.name)
    } else {
        format!("{} · 文件管理器", host.name)
    };

    let mut header_right = h_flex().flex_shrink_0().items_center().gap(px(6.));
    if let Some(cwd) = active_cwd
        && !cwd.is_empty()
        && cwd != files.current_path
    {
        let this_sync = this.clone();
        header_right = header_right.child(
            div()
                .id("files-sync-cwd")
                .test_support()
                .rounded_lg()
                .border_1()
                .border_color(palette.border)
                .bg(palette.muted_surface_bg)
                .px(px(8.))
                .py(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                .on_click(move |_, _, cx| {
                    let target = cwd.clone();
                    let _ = this_sync.update(cx, |ws, cx| {
                        ws.load_files_dir(&target, false);
                        cx.notify();
                    });
                })
                .child("同步终端目录"),
        );
    }

    if compact {
        let this_expand = this.clone();
        let expand_host_id = host.id.clone();
        header_right = header_right.child(
            div()
                .id("files-expand-full")
                .test_support()
                .rounded_lg()
                .px(px(8.))
                .py(px(4.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                .on_click(move |_, _, cx| {
                    let id = expand_host_id.clone();
                    let _ = this_expand.update(cx, |ws, cx| ws.open_files(&id, cx));
                })
                .child("全屏展开"),
        );
    } else {
        let detail_active = workspace.right_wing == Some(RightWing::Detail);
        let this_detail = this.clone();
        let btn = div()
            .id("files-toggle-detail")
            .test_support()
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded_xl()
            .border_1()
            .px(px(10.))
            .py(px(4.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_detail.update(cx, |ws, cx| {
                    ws.toggle_right_wing(RightWing::Detail, cx);
                });
            })
            .child("详情图表");
        header_right = header_right.child(if detail_active {
            btn.border_color(palette.primary_bg)
                .bg(palette.primary_bg)
                .text_color(palette.primary_fg)
        } else {
            btn.border_color(palette.border)
                .bg(palette.card_solid_bg)
                .text_color(palette.text_secondary)
                .hover(|s| s.bg(palette.subtle_bg))
        });
    }

    let this_close = this.clone();
    header_right = header_right.child(
        div()
            .id("files-close")
            .test_support()
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded_lg()
            .cursor_pointer()
            .hover(|s| s.bg(palette.subtle_bg))
            .on_click(move |_, _, cx| {
                let _ = this_close.update(cx, |ws, cx| {
                    if compact {
                        ws.right_wing = None;
                        cx.notify();
                    } else {
                        ws.close_stage(cx);
                    }
                });
            })
            .child(lucide_icon(CatalogIcon::X, 14., palette.text_faint)),
    );

    let header = h_flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(14.))
        .py(px(10.))
        .border_b_1()
        .border_color(palette.divider)
        .child(
            h_flex()
                .min_w_0()
                .items_center()
                .gap(px(8.))
                .child(lucide_icon(CatalogIcon::Folder, 16., palette.text_secondary))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_primary)
                        .truncate()
                        .child(title),
                ),
        )
        .child(header_right);

    // ---- 2. 工具条：上级 + 刷新 + 面包屑/手输路径 + 上传 + 搜索/+目录/+文件/.* ----
    let parent_opt = files.parent_path.clone();
    let has_parent = parent_opt.is_some();
    let this_up = this.clone();
    let up_btn = div()
        .id("files-btn-up")
        .test_support()
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .opacity(if has_parent { 1.0 } else { 0.4 })
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            if let Some(p) = parent_opt.clone() {
                let _ = this_up.update(cx, |ws, cx| {
                    ws.load_files_dir(&p, false);
                    cx.notify();
                });
            }
        })
        .child(lucide_icon(
            CatalogIcon::ChevronUp,
            14.,
            palette.text_secondary,
        ));

    let cur_for_refresh = files.current_path.clone();
    let this_refresh = this.clone();
    let refresh_btn = div()
        .id("files-btn-refresh")
        .test_support()
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .cursor_pointer()
        .hover(|s| s.bg(palette.subtle_bg))
        .on_click(move |_, _, cx| {
            let p = cur_for_refresh.clone();
            let _ = this_refresh.update(cx, |ws, cx| {
                ws.load_files_dir(&p, true);
                cx.notify();
            });
        })
        .child(lucide_icon(
            CatalogIcon::RefreshCw,
            13.5,
            palette.text_secondary,
        ));

    let path_bar: AnyElement = if files.editing_path {
        div()
            .id("files-path-input-wrap")
            .test_support()
            .min_w_0()
            .flex_1()
            .child(Input::new(&files.path_input))
            .into_any_element()
    } else {
        let crumbs = files.breadcrumbs(host);
        let total_crumbs = crumbs.len();
        let mut bar = h_flex()
            .id("files-breadcrumbs")
            .test_support()
            .min_w_0()
            .flex_1()
            .h(px(28.))
            .items_center()
            .gap(px(2.))
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(palette.border)
            .bg(palette.card_solid_bg)
            .px(px(8.));
        for (idx, crumb) in crumbs.into_iter().enumerate() {
            if idx > 0 {
                bar = bar.child(
                    div()
                        .px(px(2.))
                        .text_size(px(11.))
                        .text_color(palette.text_faint)
                        .child("/"),
                );
            }
            let is_last = idx + 1 == total_crumbs;
            let target = crumb.path;
            let this_crumb = this.clone();
            bar = bar.child(
                div()
                    .id(SharedString::from(format!("crumb-{idx}")))
                    .test_support()
                    .flex_shrink_0()
                    .rounded(px(4.))
                    .px(px(4.))
                    .py(px(1.))
                    .text_size(px(12.))
                    .font_weight(if is_last {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_last {
                        palette.text_primary
                    } else {
                        palette.text_secondary
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                    .on_click(move |_, _, cx| {
                        let p = target.clone();
                        let _ = this_crumb.update(cx, |ws, cx| {
                            ws.load_files_dir(&p, false);
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
                    .child(crumb.label),
            );
        }
        let this_edit_path = this.clone();
        bar = bar.child(
            div()
                .id("files-edit-path-trigger")
                .test_support()
                .min_w(px(16.))
                .flex_1()
                .h_full()
                .cursor_text()
                .on_click(move |_, window, cx| {
                    let _ = this_edit_path.update(cx, |ws, cx| ws.start_edit_file_path(window, cx));
                }),
        );
        bar.into_any_element()
    };

    let mut top_tool_row = h_flex()
        .items_center()
        .gap(px(6.))
        .child(up_btn)
        .child(refresh_btn)
        .child(path_bar);

    if files.current_path != "drives" {
        let this_upload = this.clone();
        let mut upload_btn = div()
            .id("files-btn-upload")
            .test_support()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(5.))
            .h(px(28.))
            .px(px(10.))
            .rounded_lg()
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_upload.update(cx, |ws, cx| ws.trigger_native_upload_dialog(cx));
            })
            .child(lucide_icon(CatalogIcon::Upload, 13.5, palette.primary_fg));
        if !compact {
            upload_btn = upload_btn.child("上传");
        }
        top_tool_row = top_tool_row.child(upload_btn);
    }

    let mut toolbar = v_flex()
        .flex_shrink_0()
        .gap(px(8.))
        .px(px(12.))
        .py(px(8.))
        .border_b_1()
        .border_color(palette.divider)
        .bg(palette.muted_surface_bg)
        .child(top_tool_row);

    if files.current_path != "drives" {
        let this_mkdir = this.clone();
        let this_touch = this.clone();
        let this_hidden = this.clone();
        let show_hidden = files.show_hidden;

        let hidden_btn = div()
            .id("files-btn-hidden")
            .test_support()
            .flex_shrink_0()
            .h(px(26.))
            .px(px(8.))
            .rounded_lg()
            .border_1()
            .flex()
            .items_center()
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_hidden.update(cx, |ws, cx| {
                    ws.files.show_hidden = !ws.files.show_hidden;
                    cx.notify();
                });
            })
            .child(".*");

        toolbar = toolbar.child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .child(div().min_w_0().flex_1().child(Input::new(&files.search_input)))
                .child(
                    div()
                        .id("files-btn-mkdir")
                        .test_support()
                        .flex_shrink_0()
                        .h(px(26.))
                        .px(px(8.))
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_secondary)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, window, cx| {
                            let _ = this_mkdir.update(cx, |ws, cx| {
                                ws.open_file_inline_prompt(PromptKind::Mkdir, None, window, cx);
                            });
                        })
                        .child("+目录"),
                )
                .child(
                    div()
                        .id("files-btn-touch")
                        .test_support()
                        .flex_shrink_0()
                        .h(px(26.))
                        .px(px(8.))
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_secondary)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, window, cx| {
                            let _ = this_touch.update(cx, |ws, cx| {
                                ws.open_file_inline_prompt(PromptKind::Touch, None, window, cx);
                            });
                        })
                        .child("+文件"),
                )
                .child(if show_hidden {
                    hidden_btn
                        .border_color(palette.border_selected)
                        .bg(palette.subtle_bg)
                        .text_color(palette.text_primary)
                } else {
                    hidden_btn
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .text_color(palette.text_faint)
                }),
        );
    }

    // 行内新建 / 重命名 / 删除 / 下载确认条
    if let Some(prompt) = &files.inline_prompt {
        let is_delete = prompt.kind == PromptKind::Delete;
        let this_submit = this.clone();
        let this_cancel = this.clone();
        let mut bar = h_flex()
            .id("files-inline-prompt")
            .test_support()
            .items_center()
            .gap(px(8.))
            .rounded_xl()
            .border_1()
            .border_color(palette.border_selected)
            .bg(palette.card_solid_bg)
            .p(px(8.))
            .shadow_sm()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.text_primary)
                    .child(prompt.title.clone()),
            );
        if !is_delete {
            bar = bar.child(div().min_w_0().flex_1().child(Input::new(&files.prompt_input)));
        } else {
            bar = bar.child(div().flex_1());
        }
        bar = bar.child(
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("files-prompt-confirm")
                        .test_support()
                        .rounded_lg()
                        .bg(if is_delete {
                            Hsla::from(rgb(0xdc2626))
                        } else {
                            palette.primary_bg
                        })
                        .text_color(if is_delete {
                            Hsla::from(rgb(0xffffff))
                        } else {
                            palette.primary_fg
                        })
                        .px(px(10.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_submit.update(cx, |ws, cx| ws.submit_file_inline_prompt(cx));
                        })
                        .child("确认"),
                )
                .child(
                    div()
                        .id("files-prompt-cancel")
                        .test_support()
                        .rounded_lg()
                        .px(px(8.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(palette.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, _, cx| {
                            let _ = this_cancel.update(cx, |ws, cx| {
                                ws.files.inline_prompt = None;
                                cx.notify();
                            });
                        })
                        .child("取消"),
                ),
        );
        toolbar = toolbar.child(bar);
    }

    // ---- 3. 主体区：文件列表 + 在线代码编辑器 ----
    let show_list = !(compact && files.editor_open);
    let mut main_area = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .items_stretch()
        .overflow_hidden();

    if show_list {
        main_area = main_area.child(render_file_list(workspace, this, compact, palette));
    }

    if files.editor_open {
        main_area = main_area.child(render_editor_pane(workspace, this, compact, palette));
    }

    // ---- 4. 底部传输队列面板 ----
    let host_id_for_drop = host.id.clone();
    let dir_for_drop = files.current_path.clone();
    let this_drop = this.clone();

    let mut panel = div()
        .id("file-browser-panel")
        .test_support()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.panel_bg)
        .shadow(palette.card_shadow(!compact))
        .drag_over::<ExternalPaths>(|style, _, _, _| {
            style
                .border_color(Hsla::from(rgb(0xf97316)))
                .border_dashed()
        })
        .on_drop::<ExternalPaths>(move |external, _, cx| {
            let paths = external.paths().to_vec();
            let hid = host_id_for_drop.clone();
            let dir = dir_for_drop.clone();
            let _ = this_drop.update(cx, |ws, cx| {
                ws.upload_local_paths(&hid, &dir, &paths, cx);
            });
        })
        .child(header)
        .child(toolbar)
        .child(main_area);

    if !files.transfers.is_empty() {
        panel = panel.child(render_transfers_footer(workspace, this, palette));
    }

    panel.into_any_element()
}

fn render_file_list(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    compact: bool,
    palette: &Palette,
) -> AnyElement {
    let files = &workspace.files;
    let visible = files.visible_items();
    let highlighted = files.highlighted_path().map(str::to_owned);

    let mut rows = v_flex().w_full().gap(px(2.));

    if files.loading && files.items.is_empty() {
        rows = rows.child(
            div()
                .py(px(32.))
                .text_center()
                .text_size(px(12.))
                .text_color(palette.text_faint)
                .child("正在读取目录..."),
        );
    } else {
        // 第一行：.. 返回上级目录
        if let Some(parent) = &files.parent_path {
            let parent_path = parent.clone();
            let label = if parent == "drives" {
                "（返回此电脑盘符列表）".to_owned()
            } else {
                format!("（上级: {parent}）")
            };
            let this_parent = this.clone();
            rows = rows.child(
                h_flex()
                    .id("file-row-parent")
                    .test_support()
                    .items_center()
                    .gap(px(8.))
                    .rounded_xl()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(palette.text_secondary)
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                    .on_click(move |_, _, cx| {
                        let p = parent_path.clone();
                        let _ = this_parent.update(cx, |ws, cx| {
                            ws.load_files_dir(&p, false);
                            cx.notify();
                        });
                    })
                    .child(lucide_icon(CatalogIcon::CornerLeftUp, 14., palette.text_muted))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(".."))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(palette.text_faint)
                            .child(label),
                    ),
            );
        }

        for (idx, item) in visible.iter().enumerate() {
            let entry = (*item).clone();
            let is_highlighted = highlighted.as_deref() == Some(entry.path.as_str());
            let is_editing = files.editor_open && files.editor_path == entry.path;
            let is_busy = files.busy_action_path.as_deref() == Some(entry.path.as_str());
            let is_archive = is_archive_file(&entry);

            let icon_el = if entry.is_dir {
                lucide_icon(CatalogIcon::Folder, 15., palette.text_secondary).into_any_element()
            } else if is_archive {
                lucide_icon(CatalogIcon::Archive, 15., palette.accent_amber).into_any_element()
            } else {
                lucide_icon(CatalogIcon::FileText, 15., palette.text_faint).into_any_element()
            };

            let mut left = h_flex()
                .min_w_0()
                .flex_1()
                .items_center()
                .gap(px(8.))
                .child(icon_el)
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(if is_editing {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(palette.text_primary)
                        .truncate()
                        .child(entry.name.clone()),
                );

            if is_highlighted {
                left = left.child(
                    div()
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .bg(palette.status_online)
                        .px(px(5.))
                        .py(px(1.))
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(0xffffff))
                        .child("新生成"),
                );
            } else if is_busy {
                left = left.child(
                    div()
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .bg(palette.primary_bg)
                        .px(px(5.))
                        .py(px(1.))
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.primary_fg)
                        .child("处理中..."),
                );
            }

            let mut right = h_flex().flex_shrink_0().items_center().gap(px(4.));
            if files.current_path != "drives" {
                if !entry.is_dir {
                    right = right.child(
                        div()
                            .mr(px(4.))
                            .text_size(px(11.))
                            .text_color(palette.text_faint)
                            .child(format_bytes(entry.size as f64)),
                    );
                }
                if !compact && entry.mtime > 0 {
                    right = right.child(
                        div()
                            .mr(px(6.))
                            .text_size(px(11.))
                            .text_color(palette.text_faint)
                            .child(format_mtime(entry.mtime)),
                    );
                }

                // 动作按钮组：解压 / 下载 / 压缩 / 路径 / 改名 / 删除
                if is_archive {
                    let this_extract = this.clone();
                    let ext_path = entry.path.clone();
                    let ext_name = entry.name.clone();
                    right = right.child(
                        row_action_btn(
                            format!("file-extract-{idx}"),
                            "解压",
                            Some(palette.accent_amber),
                            palette,
                        )
                        .test_support()
                        .on_click(move |_, _, cx| {
                            let p = ext_path.clone();
                            let n = ext_name.clone();
                            let _ = this_extract.update(cx, |ws, cx| {
                                if let Some(hid) = ws.files.host_id.clone() {
                                    ws.files.busy_action_path = Some(p.clone());
                                    ws.show_toast(format!("正在解压 {n}..."), ToastKind::Ok, cx);
                                    ws.api.send(ApiCommand::ExtractFile {
                                        host_id: hid,
                                        dir_to_reload: ws.files.current_path.clone(),
                                        path: p,
                                    });
                                }
                            });
                            cx.stop_propagation();
                        }),
                    );
                }

                if !entry.is_dir {
                    let this_dl = this.clone();
                    let dl_entry = entry.clone();
                    right = right.child(
                        row_action_btn(format!("file-download-{idx}"), "下载", None, palette)
                            .on_click(move |_, window, cx| {
                                let item = dl_entry.clone();
                                let _ = this_dl.update(cx, |ws, cx| {
                                    ws.open_file_inline_prompt(
                                        PromptKind::Download,
                                        Some(item),
                                        window,
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            }),
                    );
                }

                if !compact {
                    let this_comp = this.clone();
                    let comp_path = entry.path.clone();
                    let comp_name = entry.name.clone();
                    right = right.child(
                        row_action_btn(format!("file-compress-{idx}"), "压缩", None, palette)
                            .on_click(move |_, _, cx| {
                                let p = comp_path.clone();
                                let n = comp_name.clone();
                                let _ = this_comp.update(cx, |ws, cx| {
                                    if let Some(hid) = ws.files.host_id.clone() {
                                        ws.files.busy_action_path = Some(p.clone());
                                        ws.show_toast(format!("正在压缩 {n}..."), ToastKind::Ok, cx);
                                        ws.api.send(ApiCommand::CompressFile {
                                            host_id: hid,
                                            dir_to_reload: ws.files.current_path.clone(),
                                            path: p,
                                        });
                                    }
                                });
                                cx.stop_propagation();
                            }),
                    );

                    let this_copy = this.clone();
                    let copy_path = entry.path.clone();
                    right = right.child(
                        row_action_btn(format!("file-copypath-{idx}"), "路径", None, palette)
                            .on_click(move |_, _, cx| {
                                let p = copy_path.clone();
                                cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
                                let _ = this_copy.update(cx, |ws, cx| {
                                    ws.show_toast(format!("已复制路径: {p}"), ToastKind::Ok, cx);
                                });
                                cx.stop_propagation();
                            }),
                    );
                }

                let this_ren = this.clone();
                let ren_entry = entry.clone();
                right = right.child(
                    row_action_btn(format!("file-rename-{idx}"), "改名", None, palette).on_click(
                        move |_, window, cx| {
                            let item = ren_entry.clone();
                            let _ = this_ren.update(cx, |ws, cx| {
                                ws.open_file_inline_prompt(
                                    PromptKind::Rename,
                                    Some(item),
                                    window,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        },
                    ),
                );

                let this_del = this.clone();
                let del_entry = entry.clone();
                right = right.child(
                    div()
                        .id(SharedString::from(format!("file-delete-{idx}")))
                        .test_support()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .px(px(6.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(rgb(0xdc2626))
                        .cursor_pointer()
                        .hover(|s| s.bg(Hsla::from(rgb(0xfef2f2))))
                        .on_click(move |_, window, cx| {
                            let item = del_entry.clone();
                            let _ = this_del.update(cx, |ws, cx| {
                                ws.open_file_inline_prompt(
                                    PromptKind::Delete,
                                    Some(item),
                                    window,
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        })
                        .child("删除"),
                );
            }

            let this_click = this.clone();
            let click_entry = entry.clone();
            let mut row = h_flex()
                .id(SharedString::from(format!("file-row-{}", entry.name)))
                .test_support()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .rounded_xl()
                .px(px(10.))
                .py(px(6.))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let item = click_entry.clone();
                    let _ = this_click.update(cx, |ws, cx| {
                        if item.is_dir {
                            ws.load_files_dir(&item.path, false);
                            cx.notify();
                        } else {
                            ws.open_file_in_editor(&item, cx);
                        }
                    });
                })
                .child(left)
                .child(right);

            if is_highlighted {
                row = row
                    .border_1()
                    .border_color(palette.status_online)
                    .bg(palette.status_online.opacity(0.1));
            } else if is_editing {
                row = row.bg(palette.subtle_bg);
            } else {
                let subtle = palette.subtle_bg;
                row = row.hover(move |s| s.bg(subtle));
            }

            rows = rows.child(row);
        }

        if !files.loading && visible.is_empty() {
            rows = rows.child(
                div()
                    .py(px(36.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(palette.text_faint)
                    .child("当前目录为空"),
            );
        }
    }

    div()
        .id("files-list-scroll")
        .test_support()
        .flex_1()
        .h_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(8.))
        .py(px(6.))
        .child(rows)
        .into_any_element()
}

fn row_action_btn(
    id: String,
    label: &'static str,
    fill_bg: Option<Hsla>,
    palette: &Palette,
) -> Stateful<Div> {
    let base = div()
        .id(SharedString::from(id))
        .rounded(px(6.))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .child(label);
    if let Some(bg) = fill_bg {
        base.bg(bg).text_color(rgb(0xffffff))
    } else {
        let subtle = palette.subtle_bg;
        let primary = palette.text_primary;
        base.border_1()
            .border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_secondary)
            .hover(move |s| s.bg(subtle).text_color(primary))
    }
}

fn render_editor_pane(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    compact: bool,
    palette: &Palette,
) -> AnyElement {
    let files = &workspace.files;
    let this_save = this.clone();
    let this_close = this.clone();

    let header = h_flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(12.))
        .py(px(8.))
        .border_b_1()
        .border_color(palette.divider)
        .bg(palette.muted_surface_bg)
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_primary)
                        .truncate()
                        .child(files.editor_name.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(palette.text_faint)
                        .truncate()
                        .child(files.editor_path.clone()),
                ),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("editor-save-btn")
                        .test_support()
                        .rounded_lg()
                        .bg(palette.primary_bg)
                        .text_color(palette.primary_fg)
                        .px(px(12.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_save.update(cx, |ws, cx| ws.save_editor_file(cx));
                        })
                        .child(if files.editor_saving {
                            "保存中..."
                        } else {
                            "保存 (Ctrl+S)"
                        }),
                )
                .child(
                    div()
                        .id("editor-close-btn")
                        .test_support()
                        .rounded_lg()
                        .px(px(8.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(palette.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, _, cx| {
                            let _ = this_close.update(cx, |ws, cx| {
                                ws.files.editor_open = false;
                                cx.notify();
                            });
                        })
                        .child("关闭"),
                ),
        );

    let body = if files.editor_loading {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .text_color(palette.text_faint)
            .child("正在读取文件内容...")
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                Editor::new(&files.editor_state)
                    .bordered(false)
                    .size_full()
                    .text_size(px(12.)),
            )
            .into_any_element()
    };

    let mut container = div()
        .id("files-editor-pane")
        .test_support()
        .flex_1()
        .h_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(palette.card_solid_bg)
        .child(header)
        .child(body);

    if !compact {
        container = container.border_l_1().border_color(palette.divider);
    }

    container.into_any_element()
}

fn render_transfers_footer(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> impl IntoElement {
    let tasks = &workspace.files.transfers;
    let this_clear = this.clone();

    let mut list = v_flex().w_full().gap(px(6.));
    for task in tasks.iter() {
        let arrow = if task.direction == "upload" { "⬆" } else { "⬇" };
        let pct = if task.total_bytes > 0 {
            ((task.transferred_bytes as f64 * 100.0) / task.total_bytes as f64)
                .round()
                .clamp(0.0, 100.0) as f32
        } else if task.state == "completed" {
            100.0
        } else {
            30.0
        };
        let status_label = match task.state.as_str() {
            "running" => format!(
                "{} · {} / {}",
                format_rate(task.speed_bps as f64),
                format_bytes(task.transferred_bytes as f64),
                format_bytes(task.total_bytes as f64)
            ),
            "completed" => format!(
                "✓ 完成 ({}){}",
                format_rate(task.speed_bps as f64),
                if task.verified == Some(true) {
                    " 已校验"
                } else {
                    ""
                }
            ),
            _ => format!(
                "✗ 失败{}",
                task.error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            ),
        };
        let status_color = match task.state.as_str() {
            "completed" => palette.status_online,
            "failed" => palette.status_offline,
            _ => palette.text_muted,
        };
        let bar_color = match task.state.as_str() {
            "completed" => palette.status_online,
            "failed" => palette.status_offline,
            _ => palette.primary_bg,
        };

        let mut left_title = h_flex()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.text_primary)
                    .truncate()
                    .child(format!("{arrow} {}", task.filename)),
            );
        if task.resumed_from > 0 {
            left_title = left_title.child(
                div()
                    .text_size(px(10.))
                    .text_color(palette.text_faint)
                    .child(format!("续传自 {}", format_bytes(task.resumed_from as f64))),
            );
        }

        list = list.child(
            v_flex()
                .rounded_xl()
                .border_1()
                .border_color(palette.border)
                .bg(palette.card_solid_bg)
                .px(px(10.))
                .py(px(6.))
                .gap(px(4.))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(left_title)
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(10.))
                                .text_color(status_color)
                                .child(status_label),
                        ),
                )
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .overflow_hidden()
                        .rounded_full()
                        .bg(palette.subtle_bg)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(bar_color)
                                .w(relative(pct / 100.0)),
                        ),
                ),
        );
    }

    div()
        .id("files-transfers-panel")
        .test_support()
        .flex_shrink_0()
        .border_t_1()
        .border_color(palette.border)
        .bg(palette.muted_surface_bg)
        .px(px(12.))
        .py(px(8.))
        .child(
            h_flex()
                .mb(px(6.))
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_secondary)
                        .child(format!("传输队列 ({})", tasks.len())),
                )
                .child(
                    div()
                        .id("files-clear-transfers")
                        .test_support()
                        .text_color(palette.text_faint)
                        .cursor_pointer()
                        .hover(|s| s.text_color(palette.text_primary))
                        .on_click(move |_, _, cx| {
                            let _ = this_clear.update(cx, |ws, _| {
                                if let Some(hid) = ws.files.host_id.clone() {
                                    ws.api.send(ApiCommand::ClearTransfers { host_id: hid });
                                }
                            });
                        })
                        .child("清理已完成"),
                ),
        )
        .child(
            div()
                .id("files-transfers-scroll")
                .max_h(px(164.))
                .overflow_y_scroll()
                .on_scroll_wheel(|_, _, cx| {
                    cx.stop_propagation();
                })
                .child(list),
        )
}
