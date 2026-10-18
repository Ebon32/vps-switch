//! 补齐 WebView（Vue）与 GPUI 差异的核心视觉组件库：
//!
//! 1. `brand_logo`：`BrandLogo.vue` 的 1:1 还原（真实 SVG 矢量滑块 + 黑底内环 + 阴影，sm/md/lg 三档）
//! 2. `os_badge`：`OsBadge.vue` 布局 + Simple Icons 官方发行版 SVG（Ubuntu / Debian / Alpine / Windows + 两字母回退）
//! 3. `status_dot`：四态状态灯（在线绿发光 / 离线红发光 / 检测中琥珀呼吸动画 / 未知灰）
//! 4. `cpu_ring`：`HostDetailDrawer.vue` 的 52px CPU 圆环（GPUI 原生 `PathBuilder` 矢量圆弧 + 绝对居中文字层）

use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::Host;
use crate::assets::{
    BRAND_LOGO_BYTES, CatalogIcon, OS_ALPINE_BYTES, OS_DEBIAN_BYTES, OS_UBUNTU_BYTES,
    OS_WINDOWS_BYTES,
};
use crate::palette::Palette;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandLogoSize {
    /// 28x28px, rounded-lg (8px)
    Sm,
    /// 36x36px, rounded-xl (12px)
    Md,
    /// 48x48px, rounded-2xl (16px)
    Lg,
}

/// 1:1 还原 `BrandLogo.vue`：纯黑底 + 1px 白色 15% 细内边框 + 阴影 + 75% 白色双 L 滑块 SVG。
pub fn brand_logo(size_variant: BrandLogoSize) -> impl IntoElement {
    let (box_px, icon_px, radius_px) = match size_variant {
        BrandLogoSize::Sm => (28.0, 21.0, 8.0),
        BrandLogoSize::Md => (36.0, 27.0, 12.0),
        BrandLogoSize::Lg => (48.0, 36.0, 16.0),
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(box_px))
        .rounded(px(radius_px))
        .bg(rgb(0x000000))
        .border_1()
        .border_color(Hsla::from(rgb(0xffffff)).opacity(0.18))
        .shadow(vec![BoxShadow {
            color: Hsla::from(rgb(0x000000)).opacity(0.3),
            offset: point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(
            svg()
                .data(BRAND_LOGO_BYTES)
                .size(px(icon_px))
                .text_color(rgb(0xffffff)),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsBadgeSize {
    /// 24x24px（舞台展开时的紧凑侧栏卡片）
    Sm,
    /// 40x40px（仓库列表与详情铭牌）
    Md,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistroKind {
    Ubuntu,
    Debian,
    Alpine,
    Windows,
    LinuxOther,
}

pub fn detect_distro(host: &Host) -> DistroKind {
    let id = host.distro_id.as_deref().unwrap_or("").to_lowercase();
    let pretty = host.pretty_name.as_deref().unwrap_or("").to_lowercase();
    let os = host.os.as_deref().unwrap_or("").to_lowercase();
    if os.contains("win") || id.contains("win") || pretty.contains("windows") {
        return DistroKind::Windows;
    }
    if id.contains("ubuntu") || pretty.contains("ubuntu") {
        return DistroKind::Ubuntu;
    }
    if id.contains("debian") || pretty.contains("debian") {
        return DistroKind::Debian;
    }
    if id.contains("alpine") || pretty.contains("alpine") {
        return DistroKind::Alpine;
    }
    DistroKind::LinuxOther
}

/// 圆形浅灰/深灰底 + Simple Icons 官方发行版 SVG（Ubuntu #ea580c、Debian #e11d48、Alpine/Windows 中性色）+ 两字母回退。
pub fn os_badge(host: &Host, badge_size: OsBadgeSize, palette: &Palette) -> impl IntoElement {
    let (outer_px, svg_px, win_svg_px, text_px) = match badge_size {
        OsBadgeSize::Sm => (24.0, 13.0, 12.0, 10.0),
        OsBadgeSize::Md => (40.0, 18.0, 15.0, 12.0),
    };
    let distro = detect_distro(host);
    let inner: AnyElement = match distro {
        DistroKind::Ubuntu => svg()
            .data(OS_UBUNTU_BYTES)
            .size(px(svg_px))
            .text_color(rgb(0xea580c))
            .into_any_element(),
        DistroKind::Debian => svg()
            .data(OS_DEBIAN_BYTES)
            .size(px(svg_px))
            .text_color(rgb(0xe11d48))
            .into_any_element(),
        DistroKind::Alpine => svg()
            .data(OS_ALPINE_BYTES)
            .size(px(svg_px))
            .text_color(palette.badge_neutral_fg)
            .into_any_element(),
        DistroKind::Windows => svg()
            .data(OS_WINDOWS_BYTES)
            .size(px(win_svg_px))
            .text_color(palette.badge_neutral_fg)
            .into_any_element(),
        DistroKind::LinuxOther => div()
            .text_size(px(text_px))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.badge_neutral_fg)
            .child(fallback_two_letters(&host.name))
            .into_any_element(),
    };

    let id_prefix = match badge_size {
        OsBadgeSize::Sm => "os-badge-sm",
        OsBadgeSize::Md => "os-badge",
    };

    div()
        .id(SharedString::from(format!("{id_prefix}-{}", host.id)))
        .test_support()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(outer_px))
        .rounded_full()
        .border_1()
        .border_color(palette.badge_border)
        .bg(palette.badge_bg)
        .child(inner)
}

pub fn fallback_two_letters(name: &str) -> String {
    let letters: String = name
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if letters.is_empty() {
        "VP".into()
    } else {
        letters
    }
}

/// 状态灯：在线绿发光 / 离线红发光 / 检测中琥珀呼吸 / 未知灰。
pub fn status_dot(
    status: &str,
    checking: bool,
    size_px: f32,
    anim_id: impl Into<SharedString>,
    palette: &Palette,
) -> AnyElement {
    let (color, glow) = if checking {
        (palette.status_checking, true)
    } else {
        match status {
            "online" => (palette.status_online, true),
            "offline" => (palette.status_offline, true),
            _ => (palette.status_unknown, false),
        }
    };
    let mut dot = div()
        .size(px(size_px))
        .flex_shrink_0()
        .rounded_full()
        .bg(color);
    if glow {
        dot = dot.shadow(vec![BoxShadow {
            color: color.opacity(0.55),
            offset: point(px(0.), px(0.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }]);
    }
    if checking && !crate::rendering::is_software_rendering() {
        dot.with_animation(
            anim_id.into(),
            Animation::new(Duration::from_millis(1400)).repeat(),
            |el, delta| {
                let wave = (delta * std::f32::consts::PI).sin();
                el.opacity(0.35 + 0.65 * wave)
            },
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    }
}

/// 1:1 还原 `HostDetailDrawer.vue` 的 52x52px CPU 圆环：
/// r=22, stroke=4.5，从 -90°（12 点钟方向）顺时针绘制；>80% 变橙色。
/// 文字层通过 `absolute().inset_0().flex().items_center().justify_center()` 与圆环同心重合。
pub fn cpu_ring(usage_pct: f64, has_data: bool, palette: &Palette) -> impl IntoElement {
    let clamped = usage_pct.clamp(0.0, 100.0) as f32;
    let track_color = palette.divider;
    let progress_color = if clamped > 80.0 {
        palette.accent_orange
    } else {
        palette.text_primary
    };
    let label = if has_data {
        format!("{:.0}%", clamped.round())
    } else {
        "--".to_owned()
    };

    div()
        .relative()
        .size(px(52.))
        .flex_shrink_0()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    let center = bounds.center();
                    let radius = 22.0_f32;
                    let stroke_w = px(4.5);

                    if let Some(track_path) = build_arc_path(center, radius, 0.0, 1.0, stroke_w) {
                        window.paint_path(track_path, track_color);
                    }
                    if has_data && clamped > 0.5 {
                        if let Some(arc_path) =
                            build_arc_path(center, radius, 0.0, clamped / 100.0, stroke_w)
                        {
                            window.paint_path(arc_path, progress_color);
                        }
                    }
                },
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(progress_color)
                .child(label),
        )
}

fn build_arc_path(
    center: Point<Pixels>,
    radius: f32,
    start_turn: f32,
    end_turn: f32,
    stroke_width: Pixels,
) -> Option<Path<Pixels>> {
    let span = (end_turn - start_turn).clamp(0.0, 1.0);
    if span <= 0.001 {
        return None;
    }
    let steps = ((span * 64.0).ceil() as usize).max(8);
    let mut builder = PathBuilder::stroke(stroke_width);
    let start_angle = -std::f32::consts::FRAC_PI_2 + start_turn * std::f32::consts::TAU;
    let point_at = |angle: f32| -> Point<Pixels> {
        point(
            center.x + px(radius * angle.cos()),
            center.y + px(radius * angle.sin()),
        )
    };
    builder.move_to(point_at(start_angle));
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let angle = start_angle + span * t * std::f32::consts::TAU;
        builder.line_to(point_at(angle));
    }
    if span >= 0.999 {
        builder.close();
    }
    builder.build().ok()
}

/// 渲染 Lucide 图标，颜色与尺寸由调用方精确控制。
pub fn lucide_icon(name: CatalogIcon, size_px: f32, color: Hsla) -> impl IntoElement {
    Icon::new(name).size(px(size_px)).text_color(color)
}
