//! 硬件/软件渲染检测（参照 `shell-rs/src/app/rendering.rs`）。
//!
//! 在没有 3D 加速的虚拟机或远程桌面（Microsoft Basic Render Driver `0x1414`、
//! WARP、llvmpipe）里，GPUI 用 CPU 画每一帧。此时自动开启 `reduce_motion`
//! 并降低连续帧率。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::{App, GpuSpecs, Window};

static SOFTWARE_RENDERER: AtomicBool = AtomicBool::new(false);

pub const SOFTWARE_FRAME_INTERVAL: Duration = Duration::from_millis(50);

pub fn note_renderer(window: &Window, cx: &mut App) {
    let software = window.gpu_specs().is_some_and(|specs| is_software(&specs));
    SOFTWARE_RENDERER.store(software, Ordering::Relaxed);
    if software {
        cx.set_reduce_motion(true);
    }
}

pub fn is_software_rendering() -> bool {
    SOFTWARE_RENDERER.load(Ordering::Relaxed)
}

fn is_software(specs: &GpuSpecs) -> bool {
    specs.is_software_emulated || specs.driver_name.ends_with("(ID: 0x1414)")
}

#[cfg(test)]
mod tests {
    use gpui_kit::GpuSpecs;

    use super::is_software;

    #[test]
    fn detects_warp_and_basic_render_driver() {
        let specs = |emulated: bool, name: &str| GpuSpecs {
            is_software_emulated: emulated,
            driver_name: name.into(),
            ..GpuSpecs::default()
        };
        assert!(is_software(&specs(true, "llvmpipe")));
        assert!(is_software(&specs(
            false,
            "Microsoft Basic Render Driver (ID: 0x1414)"
        )));
        assert!(!is_software(&specs(false, "NVIDIA GeForce RTX 4060")));
    }
}
