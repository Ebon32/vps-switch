//! 亮/暗双色板与视觉样式 token（对齐旧前端 `App.vue` + `docs/ui-spec.md` §1）。
//!
//! GPUI 没有 CSS `dark:` 自动类切换，所有组件统一从 `Palette::for_mode(is_dark)`
//! 读取颜色，保证日夜切换一处不漏。

use gpui_kit::*;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub is_dark: bool,
    /// 页面渐变顶色（亮 `#f9fafb` / 暗 `#111215`）。
    pub page_grad_start: Hsla,
    /// 页面渐变底色（亮 `#f1f3f6` / 暗 `#090a0c`）。
    pub page_grad_end: Hsla,
    /// 顶栏毛玻璃底（亮 `white/75` / 暗 `#111215/80`）。
    pub topbar_bg: Hsla,
    /// 顶栏底边框（亮 `zinc-200/75` / 暗 `zinc-800/80`）。
    pub topbar_border: Hsla,
    /// 毛玻璃面板底（抽屉/浮层，亮 `white/88` / 暗 `#18181b/88`）。
    pub panel_bg: Hsla,
    /// 内部纯色白/黑卡片底（亮 `#ffffff` / 暗 `#18181b`）。
    pub card_solid_bg: Hsla,
    /// 卡片默认底（亮 `white/82` / 暗 `#18181b/75`）。
    pub card_bg: Hsla,
    /// 卡片悬停底（亮 `white/95` / 暗 `#27272a/90`）。
    pub card_hover_bg: Hsla,
    /// 卡片选中底（亮 `#ffffff` / 暗 `#27272a`）。
    pub card_selected_bg: Hsla,
    /// 次级浅底（胶囊/代码块/进度条轨道，亮 `#f4f4f5` / 暗 `#27272a`）。
    pub subtle_bg: Hsla,
    /// 更浅的块底（亮 `#fafafa` / 暗 `#141417`）。
    pub muted_surface_bg: Hsla,
    /// 常规细边框（亮 `zinc-200/80` / 暗 `zinc-800`）。
    pub border: Hsla,
    /// 弱分隔线（亮 `#f1f2f4` / 暗 `#27272a`）。
    pub divider: Hsla,
    /// 选中态加深边框（亮 `zinc-900/25` / 暗 `zinc-100/25`）。
    pub border_selected: Hsla,
    /// 主文字（亮 `zinc-900` `#18181b` / 暗 `zinc-100` `#f4f4f5`）。
    pub text_primary: Hsla,
    /// 次文字（亮 `zinc-600` `#52525b` / 暗 `zinc-300` `#d4d4d8`）。
    pub text_secondary: Hsla,
    /// 灰文字（亮 `zinc-500` `#71717a` / 暗 `zinc-400` `#a1a1aa`）。
    pub text_muted: Hsla,
    /// 弱文字（亮 `zinc-400` `#a1a1aa` / 暗 `zinc-500` `#71717a`）。
    pub text_faint: Hsla,
    /// 主按钮底（亮 `#18181b` / 暗 `#f4f4f5`）。
    pub primary_bg: Hsla,
    /// 主按钮文字（亮 `#ffffff` / 暗 `#18181b`）。
    pub primary_fg: Hsla,
    /// OS 徽章底（亮 `zinc-100/90` / 暗 `zinc-800/90`）。
    pub badge_bg: Hsla,
    /// OS 徽章边框（亮 `zinc-200/80` / 暗 `zinc-700/80`）。
    pub badge_border: Hsla,
    /// OS 徽章中性图标色（Alpine / Windows，亮 `zinc-700` / 暗 `zinc-300`）。
    pub badge_neutral_fg: Hsla,
    /// 新增按钮橙色 `#f97316`。
    pub accent_orange: Hsla,
    /// 收藏琥珀色 `#f59e0b`。
    pub accent_amber: Hsla,
    /// 在线翡翠绿 `#10b981`。
    pub status_online: Hsla,
    /// 离线红 `#ef4444`。
    pub status_offline: Hsla,
    /// 检测中琥珀 `#fbbf24`。
    pub status_checking: Hsla,
    /// 未知灰（亮 `zinc-300` `#d4d4d8` / 暗 `zinc-600` `#52525b`）。
    pub status_unknown: Hsla,
}

impl Palette {
    pub fn for_mode(is_dark: bool) -> Self {
        if is_dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            page_grad_start: Hsla::from(rgb(0xf9fafb)),
            page_grad_end: Hsla::from(rgb(0xf1f3f6)),
            topbar_bg: Hsla::from(rgb(0xffffff)).opacity(0.75),
            topbar_border: Hsla::from(rgb(0xe4e4e7)).opacity(0.75),
            panel_bg: Hsla::from(rgb(0xffffff)).opacity(0.88),
            card_solid_bg: Hsla::from(rgb(0xffffff)),
            card_bg: Hsla::from(rgb(0xffffff)).opacity(0.82),
            card_hover_bg: Hsla::from(rgb(0xffffff)).opacity(0.95),
            card_selected_bg: Hsla::from(rgb(0xffffff)),
            subtle_bg: Hsla::from(rgb(0xf4f4f5)),
            muted_surface_bg: Hsla::from(rgb(0xfafafa)),
            border: Hsla::from(rgb(0xe4e4e7)).opacity(0.85),
            divider: Hsla::from(rgb(0xf1f2f4)),
            border_selected: Hsla::from(rgb(0x18181b)).opacity(0.88),
            text_primary: Hsla::from(rgb(0x18181b)),
            text_secondary: Hsla::from(rgb(0x52525b)),
            text_muted: Hsla::from(rgb(0x71717a)),
            text_faint: Hsla::from(rgb(0xa1a1aa)),
            primary_bg: Hsla::from(rgb(0x18181b)),
            primary_fg: Hsla::from(rgb(0xffffff)),
            badge_bg: Hsla::from(rgb(0xf4f4f5)).opacity(0.9),
            badge_border: Hsla::from(rgb(0xe4e4e7)).opacity(0.8),
            badge_neutral_fg: Hsla::from(rgb(0x3f3f46)),
            accent_orange: Hsla::from(rgb(0xf97316)),
            accent_amber: Hsla::from(rgb(0xf59e0b)),
            status_online: Hsla::from(rgb(0x10b981)),
            status_offline: Hsla::from(rgb(0xef4444)),
            status_checking: Hsla::from(rgb(0xfbbf24)),
            status_unknown: Hsla::from(rgb(0xd4d4d8)),
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            page_grad_start: Hsla::from(rgb(0x111215)),
            page_grad_end: Hsla::from(rgb(0x090a0c)),
            topbar_bg: Hsla::from(rgb(0x111215)).opacity(0.82),
            topbar_border: Hsla::from(rgb(0x27272a)).opacity(0.85),
            panel_bg: Hsla::from(rgb(0x18181b)).opacity(0.88),
            card_solid_bg: Hsla::from(rgb(0x18181b)),
            card_bg: Hsla::from(rgb(0x18181b)).opacity(0.78),
            card_hover_bg: Hsla::from(rgb(0x27272a)).opacity(0.92),
            card_selected_bg: Hsla::from(rgb(0x27272a)),
            subtle_bg: Hsla::from(rgb(0x27272a)),
            muted_surface_bg: Hsla::from(rgb(0x141417)),
            border: Hsla::from(rgb(0x27272a)),
            divider: Hsla::from(rgb(0x27272a)).opacity(0.7),
            border_selected: Hsla::from(rgb(0xf4f4f5)).opacity(0.90),
            text_primary: Hsla::from(rgb(0xf4f4f5)),
            text_secondary: Hsla::from(rgb(0xd4d4d8)),
            text_muted: Hsla::from(rgb(0xa1a1aa)),
            text_faint: Hsla::from(rgb(0x71717a)),
            primary_bg: Hsla::from(rgb(0xf4f4f5)),
            primary_fg: Hsla::from(rgb(0x18181b)),
            badge_bg: Hsla::from(rgb(0x27272a)).opacity(0.9),
            badge_border: Hsla::from(rgb(0x3f3f46)).opacity(0.8),
            badge_neutral_fg: Hsla::from(rgb(0xd4d4d8)),
            accent_orange: Hsla::from(rgb(0xf97316)),
            accent_amber: Hsla::from(rgb(0xf59e0b)),
            status_online: Hsla::from(rgb(0x10b981)),
            status_offline: Hsla::from(rgb(0xef4444)),
            status_checking: Hsla::from(rgb(0xfbbf24)),
            status_unknown: Hsla::from(rgb(0x52525b)),
        }
    }

    /// 页面背景：自顶向下的柔和线性渐变（对应旧前端 `#f9fafb → #f1f3f6`）。
    pub fn page_background(&self) -> Background {
        linear_gradient(
            180.,
            linear_color_stop(self.page_grad_start, 0.),
            linear_color_stop(self.page_grad_end, 1.),
        )
    }

    /// 卡片阴影（默认细阴影 vs 选中柔和外扩阴影）。
    pub fn card_shadow(&self, selected: bool) -> Vec<BoxShadow> {
        let alpha = if self.is_dark { 0.42 } else { 0.10 };
        let base_alpha = if self.is_dark { 0.18 } else { 0.02 };
        if selected {
            vec![BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(alpha),
                offset: point(px(0.), px(4.)),
                blur_radius: px(18.),
                spread_radius: px(1.),
                inset: false,
            }]
        } else {
            vec![BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(base_alpha),
                offset: point(px(0.), px(1.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            }]
        }
    }
}
