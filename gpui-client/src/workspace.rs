//! 工作区：旧前端 `App.vue` 的 GPUI 对应物。
//!
//! 状态（主机、筛选、标签、舞台、右栏、主题、Toast、探针）全部集中在这里；
//! 各视觉组件（顶栏、卡片、舞台、抽屉）是读取本实体状态的渲染函数。
//! 布局与视觉规格见 `docs/ui-spec.md`。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::{Api, ApiCommand, ApiEvent, Host, Inspection, NetHistory};
use crate::terminal::TerminalView;
use crate::{detail_drawer, host_card, terminal_stage, top_bar};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Workspace,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CenterStage {
    Terminal,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightWing {
    Detail,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Row,
    Col,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    All,
    Online,
    Offline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RoleFilter {
    All,
    Primary,
    Proxy,
    Test,
    Archived,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Ok,
    Err,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: Instant,
}

pub struct OpenTab {
    pub host_id: String,
    pub mode: CenterStage,
    pub right_wing: Option<RightWing>,
    pub split: SplitDirection,
    pub panes: Vec<Entity<TerminalView>>,
    pub focused_pane: Option<usize>,
    pub creating: bool,
    pub error: Option<String>,
}

pub struct Workspace {
    api: Api,
    pub hosts: Vec<Host>,
    pub loading: bool,
    pub checking_all: bool,
    pub checking: HashSet<String>,
    pub open_tabs: Vec<OpenTab>,
    pub stage_host_id: Option<String>,
    pub center_stage: Option<CenterStage>,
    pub detail_host_id: Option<String>,
    pub right_wing: Option<RightWing>,
    pub show_sidebar_in_stage: bool,

    pub search: Entity<InputState>,
    pub search_query: String,
    pub filter_open: bool,
    pub status_filter: StatusFilter,
    pub favorite_only: bool,
    pub role_filter: RoleFilter,
    pub tag_filter: String,

    pub is_dark: bool,
    pub terminal_dark: bool,
    pub page: Page,

    pub toast: Option<Toast>,

    pub inspections: HashMap<String, Inspection>,
    pub net_history: NetHistory,
    pub inspecting_host: Option<String>,
    last_inspect: Option<Instant>,
}

impl Workspace {
    pub fn new(api: Api, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("搜索名称 / IP / 标签")
        });
        cx.observe(&search, |this, state, cx| {
            this.search_query = state.read(cx).value().to_string();
            cx.notify();
        })
        .detach();

        api.send(ApiCommand::LoadHosts);
        Self::poll(cx);
        Self {
            api,
            hosts: Vec::new(),
            loading: true,
            checking_all: false,
            checking: HashSet::new(),
            open_tabs: Vec::new(),
            stage_host_id: None,
            center_stage: None,
            detail_host_id: None,
            right_wing: None,
            show_sidebar_in_stage: false,
            search,
            search_query: String::new(),
            filter_open: false,
            status_filter: StatusFilter::All,
            favorite_only: false,
            role_filter: RoleFilter::All,
            tag_filter: String::new(),
            is_dark: false,
            terminal_dark: false,
            page: Page::Workspace,
            toast: None,
            inspections: HashMap::new(),
            net_history: NetHistory::new(),
            inspecting_host: None,
            last_inspect: None,
        }
    }

    // ---------- 查询 ----------

    pub fn host_by_id(&self, id: &str) -> Option<&Host> {
        self.hosts.iter().find(|host| host.id == id)
    }

    pub fn has_stage(&self) -> bool {
        self.center_stage.is_some() && self.stage_host_id.is_some()
    }

    pub fn stage_host(&self) -> Option<&Host> {
        self.stage_host_id.as_deref().and_then(|id| self.host_by_id(id))
    }

    pub fn right_wing_host(&self) -> Option<&Host> {
        self.detail_host_id
            .as_deref()
            .and_then(|id| self.host_by_id(id))
            .or_else(|| self.stage_host())
    }

    pub fn active_tab(&self) -> Option<&OpenTab> {
        let id = self.stage_host_id.as_deref()?;
        self.open_tabs.iter().find(|tab| tab.host_id == id)
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        let id = self.stage_host_id.as_deref()?;
        self.open_tabs.iter().position(|tab| tab.host_id == id)
    }

    pub fn online_count(&self) -> usize {
        self.hosts
            .iter()
            .filter(|host| !host.archived && host.status == "online")
            .count()
    }

    pub fn offline_count(&self) -> usize {
        self.hosts
            .iter()
            .filter(|host| !host.archived && host.status == "offline")
            .count()
    }

    pub fn total_active_count(&self) -> usize {
        self.hosts.iter().filter(|host| !host.archived).count()
    }

    pub fn favorite_count(&self) -> usize {
        self.hosts
            .iter()
            .filter(|host| !host.archived && host.favorited)
            .count()
    }

    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .hosts
            .iter()
            .filter(|host| !host.archived || self.role_filter == RoleFilter::Archived)
            .flat_map(|host| host.tags.clone())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    pub fn has_active_filter(&self) -> bool {
        self.status_filter != StatusFilter::All
            || self.favorite_only
            || self.role_filter != RoleFilter::All
            || !self.tag_filter.is_empty()
    }

    pub fn filtered_hosts(&self) -> Vec<&Host> {
        let query = self.search_query.trim().to_lowercase();
        self.hosts
            .iter()
            .filter(|host| {
                if self.role_filter == RoleFilter::Archived {
                    if !host.archived {
                        return false;
                    }
                } else {
                    if host.archived {
                        return false;
                    }
                    let role_ok = match self.role_filter {
                        RoleFilter::All => true,
                        RoleFilter::Primary => host.role.as_deref() == Some("primary"),
                        RoleFilter::Proxy => host.role.as_deref() == Some("proxy"),
                        RoleFilter::Test => host.role.as_deref() == Some("test"),
                        RoleFilter::Archived => true,
                    };
                    if !role_ok {
                        return false;
                    }
                }
                if self.favorite_only && !host.favorited {
                    return false;
                }
                match self.status_filter {
                    StatusFilter::All => {}
                    StatusFilter::Online if host.status != "online" => return false,
                    StatusFilter::Offline if host.status != "offline" => return false,
                    _ => {}
                }
                if !self.tag_filter.is_empty() && !host.tags.contains(&self.tag_filter) {
                    return false;
                }
                if query.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {} {} {}",
                    host.name,
                    host.addr,
                    host.port,
                    host.username,
                    host.pretty_name.as_deref().unwrap_or(""),
                    host.tags.join(" ")
                )
                .to_lowercase();
                haystack.contains(&query)
            })
            .collect()
    }

    // ---------- 交互 ----------

    pub fn show_toast(&mut self, text: impl Into<String>, kind: ToastKind, cx: &mut Context<Self>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            at: Instant::now(),
        });
        cx.notify();
    }

    fn replace_host(&mut self, updated: Host) {
        match self.hosts.iter_mut().find(|host| host.id == updated.id) {
            Some(existing) => *existing = updated,
            None => self.hosts.push(updated),
        }
    }

    pub fn open_terminal(&mut self, host_id: &str, cx: &mut Context<Self>) {
        self.page = Page::Workspace;
        self.sync_current_tab(cx);
        let index = match self.open_tabs.iter().position(|tab| tab.host_id == host_id) {
            Some(index) => {
                self.open_tabs[index].mode = CenterStage::Terminal;
                index
            }
            None => {
                let restore_detail = self.right_wing == Some(RightWing::Detail)
                    && self.detail_host_id.as_deref() == Some(host_id);
                self.open_tabs.push(OpenTab {
                    host_id: host_id.to_owned(),
                    mode: CenterStage::Terminal,
                    right_wing: restore_detail.then_some(RightWing::Detail),
                    split: SplitDirection::Row,
                    panes: Vec::new(),
                    focused_pane: None,
                    creating: false,
                    error: None,
                });
                self.open_tabs.len() - 1
            }
        };
        self.stage_host_id = Some(host_id.to_owned());
        self.center_stage = Some(CenterStage::Terminal);
        self.detail_host_id = Some(host_id.to_owned());
        self.right_wing = self.open_tabs[index].right_wing;
        if self.open_tabs[index].panes.is_empty() {
            self.create_pane(index, cx);
        }
        cx.notify();
    }

    pub fn open_files(&mut self, host_id: &str, cx: &mut Context<Self>) {
        self.page = Page::Workspace;
        self.sync_current_tab(cx);
        match self.open_tabs.iter().position(|tab| tab.host_id == host_id) {
            Some(index) => {
                self.open_tabs[index].mode = CenterStage::Files;
                if self.open_tabs[index].right_wing == Some(RightWing::Files) {
                    self.open_tabs[index].right_wing = None;
                }
            }
            None => {
                self.open_tabs.push(OpenTab {
                    host_id: host_id.to_owned(),
                    mode: CenterStage::Files,
                    right_wing: None,
                    split: SplitDirection::Row,
                    panes: Vec::new(),
                    focused_pane: None,
                    creating: false,
                    error: None,
                });
            }
        }
        self.stage_host_id = Some(host_id.to_owned());
        self.center_stage = Some(CenterStage::Files);
        self.detail_host_id = Some(host_id.to_owned());
        if self.right_wing == Some(RightWing::Files) {
            self.right_wing = None;
        }
        cx.notify();
    }

    pub fn toggle_detail(&mut self, host_id: &str, cx: &mut Context<Self>) {
        if self.right_wing == Some(RightWing::Detail)
            && self.detail_host_id.as_deref() == Some(host_id)
            && !self.has_stage_card_selected(host_id)
        {
            self.right_wing = None;
        } else if self.right_wing == Some(RightWing::Detail)
            && self.detail_host_id.as_deref() == Some(host_id)
        {
            self.right_wing = None;
        } else {
            self.detail_host_id = Some(host_id.to_owned());
            self.right_wing = Some(RightWing::Detail);
            self.last_inspect = None;
        }
        self.sync_current_tab(cx);
        cx.notify();
    }

    fn has_stage_card_selected(&self, host_id: &str) -> bool {
        self.has_stage() && self.stage_host_id.as_deref() == Some(host_id)
    }

    pub fn toggle_right_wing(&mut self, mode: RightWing, cx: &mut Context<Self>) {
        let Some(stage_id) = self.stage_host_id.clone() else {
            return;
        };
        if self.right_wing == Some(mode) && self.detail_host_id.as_deref() == Some(&stage_id) {
            self.right_wing = None;
        } else {
            self.detail_host_id = Some(stage_id.clone());
            self.right_wing = Some(mode);
            if mode == RightWing::Detail {
                self.last_inspect = None;
            }
        }
        self.sync_current_tab(cx);
        cx.notify();
    }

    fn sync_current_tab(&mut self, _cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_index()
            && let Some(mode) = self.center_stage
        {
            self.open_tabs[tab].mode = mode;
            self.open_tabs[tab].right_wing = self.right_wing;
        }
    }

    pub fn close_stage(&mut self, cx: &mut Context<Self>) {
        self.sync_current_tab(cx);
        self.center_stage = None;
        self.stage_host_id = None;
        if self.right_wing == Some(RightWing::Files) {
            self.right_wing = None;
        }
        cx.notify();
    }

    pub fn activate_tab(&mut self, host_id: &str, cx: &mut Context<Self>) {
        self.page = Page::Workspace;
        self.sync_current_tab(cx);
        let Some(tab) = self.open_tabs.iter().find(|tab| tab.host_id == host_id) else {
            return;
        };
        let mode = tab.mode;
        let right_wing = tab.right_wing;
        self.stage_host_id = Some(host_id.to_owned());
        self.center_stage = Some(mode);
        self.detail_host_id = Some(host_id.to_owned());
        self.right_wing = right_wing;
        if mode == CenterStage::Terminal {
            if let Some(index) = self.active_tab_index()
                && self.open_tabs[index].panes.is_empty()
            {
                self.create_pane(index, cx);
            }
        }
        cx.notify();
    }

    pub fn close_tab(&mut self, host_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self
            .open_tabs
            .iter()
            .position(|tab| tab.host_id == host_id)
        else {
            return;
        };
        self.open_tabs.remove(index);
        if self.stage_host_id.as_deref() == Some(host_id) {
            match self.open_tabs.last() {
                Some(last) => {
                    let last_id = last.host_id.clone();
                    self.activate_tab(&last_id, cx);
                }
                None => self.close_stage(cx),
            }
        }
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.show_sidebar_in_stage = !self.show_sidebar_in_stage;
        cx.notify();
    }

    /// 把当前主机的终端弹到独立系统窗口（旧前端的「独立窗口」按钮）。
    pub fn popout_terminal(&mut self, host_id: &str, cx: &mut Context<Self>) {
        self.api.send(ApiCommand::Post {
            path: "/api/window/open-terminal".into(),
            body: serde_json::json!({ "hostId": host_id }),
        });
        let name = self
            .host_by_id(host_id)
            .map(|host| host.name.clone())
            .unwrap_or_else(|| host_id.to_owned());
        self.show_toast(
            format!("已在独立原生窗口打开「{name}」终端"),
            ToastKind::Ok,
            cx,
        );
    }

    pub fn toggle_terminal_dark(&mut self, cx: &mut Context<Self>) {
        self.terminal_dark = !self.terminal_dark;
        let dark = self.terminal_dark;
        for tab in &self.open_tabs {
            for pane in &tab.panes {
                pane.update(cx, |view, cx| view.set_dark(dark, cx));
            }
        }
        cx.notify();
    }

    pub fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.is_dark = !self.is_dark;
        self.terminal_dark = self.is_dark;
        let mode = if self.is_dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        Theme::change(mode, None, cx);
        let dark = self.terminal_dark;
        for tab in &self.open_tabs {
            for pane in &tab.panes {
                pane.update(cx, |view, cx| view.set_dark(dark, cx));
            }
        }
        cx.notify();
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.page = if self.page == Page::Settings {
            Page::Workspace
        } else {
            Page::Settings
        };
        cx.notify();
    }

    pub fn refresh_hosts(&mut self, cx: &mut Context<Self>) {
        self.api.send(ApiCommand::LoadHosts);
        cx.notify();
    }

    /// 立即采样一次探针（详情抽屉的「刷新」）。
    pub fn inspect_now(&mut self, host_id: &str, cx: &mut Context<Self>) {
        self.inspecting_host = Some(host_id.to_owned());
        self.last_inspect = Some(Instant::now());
        self.api.send(ApiCommand::Inspect(host_id.to_owned()));
        cx.notify();
    }

    pub fn check_all(&mut self, cx: &mut Context<Self>) {
        if self.checking_all {
            return;
        }
        self.checking_all = true;
        self.show_toast("正在并发探索全部主机在线状态与实时硬件快照...", ToastKind::Ok, cx);
        self.api.send(ApiCommand::CheckAll);
        cx.notify();
    }

    pub fn patch_meta(&mut self, host_id: &str, patch: serde_json::Value, cx: &mut Context<Self>) {
        self.api.send(ApiCommand::PatchMeta {
            host_id: host_id.to_owned(),
            patch,
        });
        cx.notify();
    }

    pub fn toggle_favorite(&mut self, host_id: &str, cx: &mut Context<Self>) {
        let Some(host) = self.host_by_id(host_id) else {
            return;
        };
        let favorited = !host.favorited;
        self.patch_meta(host_id, serde_json::json!({"favorited": favorited}), cx);
    }

    pub fn toggle_pinned(&mut self, host_id: &str, cx: &mut Context<Self>) {
        let Some(host) = self.host_by_id(host_id) else {
            return;
        };
        let pinned = !host.pinned;
        self.patch_meta(host_id, serde_json::json!({"pinned": pinned}), cx);
    }

    /// 拖拽排序：把 `from_id` 挪到 `to_id` 的位置（本机不参与）。
    pub fn reorder(&mut self, from_id: &str, to_id: &str, cx: &mut Context<Self>) {
        if from_id == to_id {
            return;
        }
        let mut ids: Vec<String> = self
            .hosts
            .iter()
            .filter(|host| host.kind != "local")
            .map(|host| host.id.clone())
            .collect();
        let (Some(from), Some(to)) = (
            ids.iter().position(|id| id == from_id),
            ids.iter().position(|id| id == to_id),
        ) else {
            return;
        };
        let moved = ids.remove(from);
        ids.insert(to, moved);
        self.api.send(ApiCommand::Reorder { ordered_ids: ids });
        cx.notify();
    }

    pub fn create_pane(&mut self, tab_index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.open_tabs.get(tab_index) else {
            return;
        };
        if tab.creating {
            return;
        }
        let host_id = tab.host_id.clone();
        let dark = self.terminal_dark;
        let pane = cx.new(|cx| TerminalView::new(host_id, dark, cx));
        if let Some(tab) = self.open_tabs.get_mut(tab_index) {
            tab.panes.push(pane);
            tab.focused_pane = Some(tab.panes.len() - 1);
            tab.creating = false;
            tab.error = None;
        }
        cx.notify();
    }

    pub fn split(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        self.open_tabs[index].split = direction;
        self.create_pane(index, cx);
    }

    pub fn close_pane(&mut self, tab_index: usize, pane_index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.open_tabs.get_mut(tab_index) else {
            return;
        };
        if pane_index >= tab.panes.len() {
            return;
        }
        tab.panes.remove(pane_index);
        tab.focused_pane = if tab.panes.is_empty() {
            None
        } else {
            Some(pane_index.min(tab.panes.len() - 1))
        };
        if tab.panes.is_empty() {
            let host_id = tab.host_id.clone();
            self.close_tab(&host_id, cx);
        } else {
            cx.notify();
        }
    }

    pub fn focus_pane(&mut self, tab_index: usize, pane_index: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.open_tabs.get_mut(tab_index) {
            tab.focused_pane = Some(pane_index);
            cx.notify();
        }
    }

    // ---------- 事件轮询 ----------

    fn poll(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let alive = this.update(cx, |this, cx| this.tick(cx));
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for event in self.api.try_recv() {
            changed = true;
            match event {
                ApiEvent::Hosts(hosts) => {
                    self.hosts = hosts;
                    self.loading = false;
                }
                ApiEvent::HostsFailed(error) => {
                    self.loading = false;
                    self.toast = Some(Toast {
                        text: format!("读取主机列表失败: {error}"),
                        kind: ToastKind::Err,
                        at: Instant::now(),
                    });
                }
                ApiEvent::Inspection {
                    host_id,
                    inspection,
                    host,
                } => {
                    let history = self.net_history.entry(host_id.clone()).or_default();
                    history.push((inspection.net_rx_bps, inspection.net_tx_bps));
                    if history.len() > 20 {
                        history.remove(0);
                    }
                    self.inspections.insert(host_id, inspection);
                    if let Some(host) = host {
                        self.replace_host(host);
                    }
                    self.inspecting_host = None;
                }
                ApiEvent::CheckedAll(hosts) => {
                    let online = hosts
                        .iter()
                        .filter(|host| !host.archived && host.status == "online")
                        .count();
                    self.hosts = hosts;
                    self.checking_all = false;
                    self.toast = Some(Toast {
                        text: format!("机队快照同步完成：{online} 台主机在线"),
                        kind: ToastKind::Ok,
                        at: Instant::now(),
                    });
                }
                ApiEvent::HostUpdated(host) => self.replace_host(host),
                ApiEvent::Failed(error) => {
                    self.inspecting_host = None;
                    self.toast = Some(Toast {
                        text: error,
                        kind: ToastKind::Err,
                        at: Instant::now(),
                    });
                }
                ApiEvent::Json(_) => {}
            }
        }

        // 详情抽屉打开时每 2.8s 采样一次。
        if self.right_wing == Some(RightWing::Detail)
            && let Some(host_id) = self.detail_host_id.clone()
        {
            let due = self
                .last_inspect
                .map(|at| at.elapsed() >= Duration::from_millis(2800))
                .unwrap_or(true);
            if due && self.inspecting_host.is_none() {
                self.last_inspect = Some(Instant::now());
                self.inspecting_host = Some(host_id.clone());
                self.api.send(ApiCommand::Inspect(host_id));
                changed = true;
            }
        }

        // Toast 2.8s 自动消失。
        if let Some(toast) = &self.toast
            && toast.at.elapsed() >= Duration::from_millis(2800)
        {
            self.toast = None;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let has_stage = self.has_stage();
        let main = match self.page {
            Page::Settings => div()
                .flex_1()
                .min_h_0()
                .child(crate::settings_page::render(self, &this, cx))
                .into_any_element(),
            Page::Workspace => {
                let sidebar = if has_stage {
                    if self.show_sidebar_in_stage {
                        div()
                            .w(px(336.))
                            .flex_shrink_0()
                            .h_full()
                            .min_h_0()
                            .child(host_card::render_list(self, &this, cx))
                            .into_any_element()
                    } else {
                        div().w(px(0.)).into_any_element()
                    }
                } else {
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(1152.))
                        .child(host_card::render_list(self, &this, cx))
                        .into_any_element()
                };
                let center = if has_stage {
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(terminal_stage::render(self, &this, cx))
                        .into_any_element()
                } else {
                    div().w(px(0.)).into_any_element()
                };
                let right = if self.right_wing.is_some() && self.right_wing_host().is_some() {
                    div()
                        .w(px(336.))
                        .flex_shrink_0()
                        .h_full()
                        .min_h_0()
                        .child(detail_drawer::render(self, &this, cx))
                        .into_any_element()
                } else {
                    div().w(px(0.)).into_any_element()
                };
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_stretch()
                    .justify_center()
                    .gap(px(14.))
                    .px(px(16.))
                    .py(px(16.))
                    .child(sidebar)
                    .child(center)
                    .child(right)
                    .into_any_element()
            }
        };

        let mut root = div()
            .v_flex()
            .size_full()
            .bg(page_background())
            .text_color(cx.theme().foreground)
            .child(top_bar::render(self, &this, cx))
            .child(main);

        if let Some(toast) = &self.toast {
            let (bg, fg, border) = match toast.kind {
                ToastKind::Ok => (rgb(0x18181b), rgb(0xffffff), rgb(0x3f3f46)),
                ToastKind::Err => (rgb(0xffffff), rgb(0xdc2626), rgb(0xfecaca)),
            };
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(20.))
                    .right(px(20.))
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_color(fg)
                    .px(px(14.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .shadow_lg()
                    .child(toast.text.clone()),
            );
        }
        root
    }
}

/// 页面底色：旧前端是径向渐变 `#f9fafb → #f1f3f6`；GPUI 没有径向渐变，
/// 用两端的中间值近似。
fn page_background() -> Hsla {
    Hsla::from(rgb(0xf5f6f8))
}
