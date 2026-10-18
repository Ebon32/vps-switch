//! 应用静态资源包（`AssetSource`）。
//!
//! 挂载：
//! 1. `BrandLogo.vue` 的双 L 滑块品牌 SVG
//! 2. Simple Icons 官方发行版 OS 图标（Ubuntu / Debian / Alpine / Windows）
//! 3. `gpui_kit::assets::AllAssets` 全量 Lucide 图标库

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

pub use gpui_kit::assets::IconName as CatalogIcon;

pub const BRAND_LOGO_ICON: &str = "icons/brand/logo.svg";
pub const BRAND_LOGO_BYTES: &[u8] = include_bytes!("../assets/icons/brand/logo.svg");

pub const OS_UBUNTU_ICON: &str = "icons/os/ubuntu.svg";
pub const OS_UBUNTU_BYTES: &[u8] = include_bytes!("../assets/icons/os/ubuntu.svg");

pub const OS_DEBIAN_ICON: &str = "icons/os/debian.svg";
pub const OS_DEBIAN_BYTES: &[u8] = include_bytes!("../assets/icons/os/debian.svg");

pub const OS_ALPINE_ICON: &str = "icons/os/alpine.svg";
pub const OS_ALPINE_BYTES: &[u8] = include_bytes!("../assets/icons/os/alpine.svg");

pub const OS_WINDOWS_ICON: &str = "icons/os/windows.svg";
pub const OS_WINDOWS_BYTES: &[u8] = include_bytes!("../assets/icons/os/windows.svg");

pub const CUSTOM_SVGS: &[(&str, &[u8])] = &[
    (BRAND_LOGO_ICON, BRAND_LOGO_BYTES),
    (OS_UBUNTU_ICON, OS_UBUNTU_BYTES),
    (OS_DEBIAN_ICON, OS_DEBIAN_BYTES),
    (OS_ALPINE_ICON, OS_ALPINE_BYTES),
    (OS_WINDOWS_ICON, OS_WINDOWS_BYTES),
];

#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = CUSTOM_SVGS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::AllAssets.list(path)?;
        paths.extend(
            CUSTOM_SVGS
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{AssetSource as _, DevicePixels, Size, SvgRenderer, SvgSize};

    use super::{AppAssets, CUSTOM_SVGS};

    #[test]
    fn all_custom_svgs_load_and_rasterize_with_non_empty_pixels() {
        let renderer = SvgRenderer::new(Arc::new(AppAssets));
        for (path, _) in CUSTOM_SVGS {
            let bytes = AppAssets
                .load(path)
                .unwrap_or_else(|err| panic!("{path} 加载失败: {err}"))
                .unwrap_or_else(|| panic!("{path} 未注册在 AppAssets 中"));
            assert!(bytes.starts_with(b"<svg"), "{path} 内容不是合法 SVG");

            let parsed = renderer
                .parse_svg(&bytes)
                .unwrap_or_else(|err| panic!("{path} usvg 解析失败: {err}"));
            let image = renderer
                .render_parsed(
                    &parsed,
                    SvgSize::ExactSize(Size::new(DevicePixels(48), DevicePixels(48))),
                )
                .unwrap_or_else(|err| panic!("{path} resvg 光栅化失败: {err}"));
            assert_eq!(image.size(0), Size::new(DevicePixels(48), DevicePixels(48)));
            let alpha_bytes = image
                .as_bytes(0)
                .expect("rendered SVG frame must have pixel buffer");
            let non_transparent = alpha_bytes.chunks_exact(4).filter(|px| px[3] > 0).count();
            assert!(
                non_transparent > 10,
                "{path} 光栅化后全部透明（有效像素数={non_transparent}）"
            );
        }
    }
}
