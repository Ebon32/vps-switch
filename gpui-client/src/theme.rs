//! 应用主题：把 gpui-kit 的浅色主题换成 vps-switch 的白色毛玻璃配色。
//!
//! 色板在 `assets/theme-light.json`（由内置 Default Light 派生，配色见
//! `docs/ui-spec.md` §1）。GPUI 没有背景模糊，毛玻璃观感靠半透明白 + 细边框
//! + 阴影近似；真正的模糊留给后续增强。

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode};
use gpui_kit::*;

pub fn install(cx: &mut App) {
    // 首次调用会创建全局 Theme 并装载默认的亮/暗主题。
    Theme::change(ThemeMode::Light, None, cx);
    let config: ThemeConfig =
        serde_json::from_str(include_str!("../assets/theme-light.json")).expect("theme json");
    cx.global_mut::<Theme>().light_theme = Rc::new(config);
    Theme::change(ThemeMode::Light, None, cx);
}
