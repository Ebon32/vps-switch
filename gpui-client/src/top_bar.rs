//! 顶栏（对齐 `App.vue` 1518-1841 行 + `docs/ui-spec.md` §3）。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::widgets::{BrandLogoSize, brand_logo, lucide_icon, status_dot};
use crate::workspace::{OpenTab, Page, RoleFilter, StatusFilter, Workspace};

pub fn render(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    _cx: &App,
) -> impl IntoElement {
    let has_stage = workspace.has_stage();

    let mut left = h_flex().min_w_0().flex_1().items_center().gap(px(6.));

    // 1. 品牌 Logo + 标题
    left = left.child(
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .mr(px(4.))
            .child(brand_logo(BrandLogoSize::Sm))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.text_primary)
                    .child("VPS Switch"),
            ),
    );

    // 2. 「⌂ 仓库」按钮
    let home_active = workspace.page == Page::Workspace && !has_stage;
    let this_home = this.clone();
    left = left.child(
        raised_button(home_active, palette)
            .id("nav-home")
            .test_support()
            .on_click(move |_, _, cx| {
                let _ = this_home.update(cx, |w, cx| w.close_stage(cx));
            })
            .child(lucide_icon(
                CatalogIcon::House,
                14.,
                if home_active {
                    palette.text_primary
                } else {
                    palette.text_muted
                },
            ))
            .child("仓库")
            .child(count_badge(
                format!(
                    "{}/{}",
                    workspace.online_count(),
                    workspace.total_active_count()
                ),
                palette,
            )),
    );

    // 3. 「◫ 侧栏」显隐按钮（仅在打开终端或文件舞台时显示）
    if workspace.page == Page::Workspace && has_stage {
        let side_active = workspace.show_sidebar_in_stage;
        let this_side = this.clone();
        left = left.child(
            raised_button(side_active, palette)
                .id("nav-sidebar")
                .test_support()
                .on_click(move |_, _, cx| {
                    let _ = this_side.update(cx, |w, cx| w.toggle_sidebar(cx));
                })
                .child(lucide_icon(
                    CatalogIcon::PanelLeft,
                    14.,
                    if side_active {
                        palette.text_primary
                    } else {
                        palette.text_muted
                    },
                ))
                .child(if side_active {
                    "收起侧栏"
                } else {
                    "展开侧栏"
                }),
        );
    }

    // 4. 已打开的会话标签页
    for tab in &workspace.open_tabs {
        left = left.child(session_tab(workspace, tab, this, palette));
    }

    // ---- 右组 ----
    let mut right = h_flex().flex_shrink_0().items_center().gap(px(8.));

    if workspace.page == Page::Workspace {
        // 搜索框
        right = right.child(
            div()
                .id("nav-search-box")
                .test_support()
                .w(px(192.))
                .child(Input::new(&workspace.search)),
        );

        // 筛选按钮 + 下拉浮层
        let active_filter = workspace.has_active_filter();
        let this_filter = this.clone();
        let this_clear_filter = this.clone();
        let label = filter_summary(workspace);
        let fg = if active_filter {
            palette.primary_fg
        } else {
            palette.text_secondary
        };

        let mut filter_btn = pill_button(active_filter, palette)
            .id("nav-filter")
            .test_support()
            .on_click(move |_, _, cx| {
                let _ = this_filter.update(cx, |w, cx| {
                    w.filter_open = !w.filter_open;
                    cx.notify();
                });
            })
            .child(lucide_icon(CatalogIcon::Funnel, 13.5, fg))
            .child(label);

        if active_filter {
            filter_btn = filter_btn.child(
                div()
                    .id("nav-filter-clear-inline")
                    .test_support()
                    .ml(px(2.))
                    .px(px(4.))
                    .rounded_full()
                    .on_click(move |_, _, cx| {
                        let _ = this_clear_filter.update(cx, |w, cx| {
                            w.reset_filters();
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
                    .child("×"),
            );
        }

        let mut wrapper = div().relative().child(filter_btn);
        if workspace.filter_open {
            wrapper = wrapper.child(filter_panel(workspace, this, palette));
        }
        right = right.child(wrapper);

        // 检测在线按钮
        let checking = workspace.checking_all;
        let this_check = this.clone();
        right = right.child(
            pill_button(false, palette)
                .id("nav-check")
                .test_support()
                .on_click(move |_, _, cx| {
                    let _ = this_check.update(cx, |w, cx| w.check_all(cx));
                })
                .child(lucide_icon(
                    CatalogIcon::RefreshCw,
                    13.5,
                    palette.text_secondary,
                ))
                .child(if checking { "检测中" } else { "检测在线" }),
        );
    }

    // 昼夜主题切换圆钮
    let this_theme = this.clone();
    let is_dark = workspace.is_dark;
    right = right.child(
        round_button(false, palette)
            .id("nav-theme")
            .test_support()
            .on_click(move |_, _, cx| {
                let _ = this_theme.update(cx, |w, cx| w.toggle_theme(cx));
            })
            .child(lucide_icon(
                if is_dark {
                    CatalogIcon::Sun
                } else {
                    CatalogIcon::Moon
                },
                15.,
                palette.text_secondary,
            )),
    );

    // 设置圆钮
    let this_settings = this.clone();
    let settings_active = workspace.page == Page::Settings;
    right = right.child(
        round_button(settings_active, palette)
            .id("nav-settings")
            .test_support()
            .on_click(move |_, _, cx| {
                let _ = this_settings.update(cx, |w, cx| w.open_settings(cx));
            })
            .child(lucide_icon(
                CatalogIcon::Settings,
                15.,
                if settings_active {
                    palette.primary_fg
                } else {
                    palette.text_secondary
                },
            )),
    );

    // 新增 VPS 橙色加号
    let this_add = this.clone();
    right = right.child(
        div()
            .id("nav-add")
            .test_support()
            .flex()
            .items_center()
            .justify_center()
            .size(px(32.))
            .rounded_full()
            .bg(palette.accent_orange)
            .text_color(rgb(0xffffff))
            .shadow_sm()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0xea580c)))
            .on_click(move |_, window, cx| {
                let _ = this_add.update(cx, |w, cx| w.open_create_modal(window, cx));
            })
            .child(lucide_icon(CatalogIcon::Plus, 16., Hsla::from(rgb(0xffffff)))),
    );

    h_flex()
        .h(px(52.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(16.))
        .py(px(8.))
        .border_b_1()
        .border_color(palette.topbar_border)
        .bg(palette.topbar_bg)
        .child(left)
        .child(right)
}

fn session_tab(
    workspace: &Workspace,
    tab: &OpenTab,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> impl IntoElement {
    let host = workspace.host_by_id(&tab.host_id);
    let name = host
        .map(|h| h.name.clone())
        .unwrap_or_else(|| tab.host_id.clone());
    let status = host
        .map(|h| h.status.as_str())
        .unwrap_or("unknown");
    let active = workspace.page == Page::Workspace
        && workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(tab.host_id.as_str());
    let pane_count = tab.panes.len();
    let host_id = tab.host_id.clone();

    let this_activate = this.clone();
    let this_close = this.clone();
    let close_id = host_id.clone();

    let base = h_flex()
        .id(SharedString::from(format!("session-{}", host_id)))
        .test_support()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.))
        .h(px(29.))
        .px(px(12.))
        .rounded_xl()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            let id = host_id.clone();
            let _ = this_activate.update(cx, |w, cx| w.activate_tab(&id, cx));
        })
        .child(status_dot(
            status,
            false,
            8.0,
            format!("tab-dot-{}", tab.host_id),
            palette,
        ))
        .child(div().max_w(px(128.)).truncate().child(name));

    let base = if pane_count > 1 {
        base.child(count_badge(format!("×{pane_count}"), palette))
    } else {
        base
    };

    let styled = if active {
        base.border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_primary)
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .bg(palette.subtle_bg.opacity(0.6))
            .text_color(palette.text_secondary)
    };

    styled.child(
        div()
            .id(SharedString::from(format!("session-close-{}", close_id)))
            .test_support()
            .flex()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded_sm()
            .text_color(palette.text_faint)
            .hover(|s| s.bg(palette.divider).text_color(palette.text_primary))
            .on_click(move |_, _, cx| {
                let id = close_id.clone();
                let _ = this_close.update(cx, |w, cx| w.close_tab(&id, cx));
                cx.stop_propagation();
            })
            .child("×"),
    )
}

fn raised_button(active: bool, palette: &Palette) -> Div {
    let base = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(29.))
        .px(px(12.))
        .rounded_xl()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_primary)
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .text_color(palette.text_muted)
            .hover(|s| s.bg(palette.card_bg).text_color(palette.text_primary))
    }
}

fn pill_button(active: bool, palette: &Palette) -> Div {
    let base = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(29.))
        .px(px(12.))
        .rounded_full()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(palette.primary_bg)
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
            .shadow_sm()
    } else {
        base.border_color(palette.border)
            .bg(palette.card_bg)
            .text_color(palette.text_secondary)
            .hover(|s| s.bg(palette.card_solid_bg))
    }
}

fn round_button(active: bool, palette: &Palette) -> Div {
    let base = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .cursor_pointer();
    if active {
        base.border_color(palette.primary_bg)
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
            .shadow_sm()
    } else {
        base.border_color(palette.border)
            .bg(palette.card_bg)
            .text_color(palette.text_secondary)
            .hover(|s| s.bg(palette.card_solid_bg).text_color(palette.text_primary))
    }
}

fn count_badge(text: String, palette: &Palette) -> impl IntoElement {
    div()
        .rounded_full()
        .bg(palette.subtle_bg)
        .px(px(6.))
        .py(px(1.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_muted)
        .child(text)
}

fn filter_summary(workspace: &Workspace) -> String {
    let mut parts: Vec<String> = Vec::new();
    if workspace.favorite_only {
        parts.push("★ 收藏".into());
    }
    match workspace.status_filter {
        StatusFilter::Online => parts.push("在线".into()),
        StatusFilter::Offline => parts.push("离线".into()),
        StatusFilter::All => {}
    }
    match workspace.role_filter {
        RoleFilter::Primary => parts.push("主力".into()),
        RoleFilter::Proxy => parts.push("代理".into()),
        RoleFilter::Test => parts.push("测试".into()),
        RoleFilter::Archived => parts.push("归档".into()),
        RoleFilter::All => {}
    }
    if !workspace.tag_filter.is_empty() {
        parts.push(format!("#{}", workspace.tag_filter));
    }
    if parts.is_empty() {
        "筛选".into()
    } else {
        format!("筛选 · {}", parts.join(" · "))
    }
}

fn filter_panel(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> AnyElement {
    let mut panel = v_flex()
        .id("filter-dropdown-panel")
        .test_support()
        .absolute()
        .top(px(37.))
        .right(px(0.))
        .w(px(256.))
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .p(px(14.))
        .shadow_xl()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        });

    let mut head = h_flex().items_center().justify_between().child(
        div()
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.text_faint)
            .child("在线状态"),
    );
    if workspace.has_active_filter() {
        let this_reset = this.clone();
        head = head.child(
            div()
                .id("filter-reset")
                .test_support()
                .text_size(px(11.))
                .text_color(palette.text_muted)
                .cursor_pointer()
                .hover(|s| s.text_color(palette.text_primary))
                .on_click(move |_, _, cx| {
                    let _ = this_reset.update(cx, |w, cx| {
                        w.reset_filters();
                        cx.notify();
                    });
                })
                .child("重置全部"),
        );
    }
    panel = panel.child(head);

    let mut grid = h_flex().mt(px(6.)).gap(px(4.));
    let statuses: [(&'static str, &'static str, StatusFilter, Option<Hsla>); 3] = [
        ("全部", "all", StatusFilter::All, None),
        (
            "在线",
            "online",
            StatusFilter::Online,
            Some(palette.status_online),
        ),
        (
            "离线",
            "offline",
            StatusFilter::Offline,
            Some(palette.status_offline),
        ),
    ];
    for (label, slug, filter, dot) in statuses {
        let active = workspace.status_filter == filter;
        let text = match filter {
            StatusFilter::Online => format!("{label} ({})", workspace.online_count()),
            StatusFilter::Offline => format!("{label} ({})", workspace.offline_count()),
            StatusFilter::All => label.to_owned(),
        };
        let this_status = this.clone();
        let mut chip = filter_chip(active, palette)
            .id(SharedString::from(format!("filter-status-{slug}")))
            .test_support()
            .on_click(move |_, _, cx| {
                let _ = this_status.update(cx, |w, cx| {
                    w.status_filter = filter;
                    cx.notify();
                });
            });
        if let Some(dot_color) = dot {
            chip = chip.child(div().size(px(8.)).rounded_full().bg(dot_color));
        }
        grid = grid.child(chip.child(text));
    }
    panel = panel.child(grid);

    // 收藏与分类
    panel = panel.child(
        v_flex()
            .mt(px(12.))
            .pt(px(12.))
            .border_t_1()
            .border_color(palette.divider)
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_faint)
                    .child("收藏与分类"),
            ),
    );

    let mut chips = h_flex().mt(px(6.)).flex_wrap().gap(px(4.));
    {
        let active = workspace.favorite_only;
        let this_fav = this.clone();
        let base = div()
            .id("filter-favorite")
            .test_support()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(24.))
            .px(px(10.))
            .rounded_xl()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_fav.update(cx, |w, cx| {
                    w.favorite_only = !w.favorite_only;
                    cx.notify();
                });
            })
            .child(format!("★ 收藏 ({})", workspace.favorite_count()));
        chips = chips.child(if active {
            base.bg(palette.accent_amber).text_color(rgb(0xffffff))
        } else {
            base.bg(palette.subtle_bg)
                .text_color(palette.text_secondary)
        });
    }

    let roles: [(&'static str, &'static str, RoleFilter); 5] = [
        ("全部分类", "all", RoleFilter::All),
        ("主力", "primary", RoleFilter::Primary),
        ("代理", "proxy", RoleFilter::Proxy),
        ("测试", "test", RoleFilter::Test),
        ("归档", "archived", RoleFilter::Archived),
    ];
    for (label, slug, role) in roles {
        let active = workspace.role_filter == role;
        let this_role = this.clone();
        let base = div()
            .id(SharedString::from(format!("filter-role-{slug}")))
            .test_support()
            .flex()
            .items_center()
            .h(px(24.))
            .px(px(10.))
            .rounded_xl()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_role.update(cx, |w, cx| {
                    w.role_filter = role;
                    cx.notify();
                });
            })
            .child(label);
        chips = chips.child(if active {
            base.bg(palette.primary_bg).text_color(palette.primary_fg)
        } else {
            base.bg(palette.subtle_bg)
                .text_color(palette.text_secondary)
        });
    }
    panel = panel.child(chips);

    // 标签
    let tags = workspace.all_tags();
    if !tags.is_empty() {
        let mut tag_chips = h_flex().flex_wrap().gap(px(4.));
        for tag in tags {
            let active = workspace.tag_filter == tag;
            let this_tag = this.clone();
            let value = tag.clone();
            let base = div()
                .id(SharedString::from(format!("filter-tag-{}", tag)))
                .test_support()
                .flex()
                .items_center()
                .h(px(22.))
                .px(px(8.))
                .rounded_lg()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let value = value.clone();
                    let _ = this_tag.update(cx, |w, cx| {
                        w.tag_filter = if w.tag_filter == value {
                            String::new()
                        } else {
                            value
                        };
                        cx.notify();
                    });
                })
                .child(format!("#{tag}"));
            tag_chips = tag_chips.child(if active {
                base.bg(palette.primary_bg).text_color(palette.primary_fg)
            } else {
                base.bg(palette.subtle_bg)
                    .text_color(palette.text_secondary)
            });
        }
        panel = panel.child(
            v_flex()
                .mt(px(12.))
                .pt(px(12.))
                .border_t_1()
                .border_color(palette.divider)
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_faint)
                        .child("标签"),
                )
                .child(tag_chips),
        );
    }

    panel.into_any_element()
}

fn filter_chip(active: bool, palette: &Palette) -> Div {
    let base = div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .py(px(6.))
        .px(px(8.))
        .rounded_xl()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.bg(palette.primary_bg).text_color(palette.primary_fg)
    } else {
        base.bg(palette.subtle_bg)
            .text_color(palette.text_secondary)
    }
}
