//! 无头 UI 测试：真实面板在测试窗口里渲染，用真实指针事件驱动。
//!
//! 注意：不要 `use gpui_kit::*`——glob 会把 gpui 的 `test` 宏带进作用域，
//! 遮蔽 Rust 内置的 `#[test]` 属性（gpui-kit 文档明确提醒过）。这里全部
//! 显式导入。
//!
//! 为什么不用 `gpui_kit::test::TestWindowExt`：gpui-kit 的 test-support
//! feature 会拉入 gpui-fast 系列，与 gpui-pre 0.3.9 的依赖树版本冲突
//! （cocoa / unicode-properties），解析直接失败。下面的 helper 是
//! TestWindowExt 同一套底层调用的精简版（公共 GPUI API）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::{
    App, AppContext as _, ElementId, Entity, InputEvent as _, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, TestAppContext, Window,
    WindowHandle, px, size,
};

use vps_switch_gpui::api::HostSummary;
use vps_switch_gpui::hosts::{HostList, HostListEvent};

fn host(id: &str, name: &str, online: bool) -> HostSummary {
    HostSummary {
        id: id.into(),
        name: name.into(),
        addr: "10.0.0.1".into(),
        kind: "ssh".into(),
        os: Some("linux".into()),
        pretty_name: Some("Ubuntu 24.04".into()),
        pinned: false,
        favorited: false,
        latency_ms: online.then_some(42),
        last_error: None,
    }
}

fn render_frame(window: &mut Window, cx: &mut App) {
    window.refresh();
    window.draw(cx).clear(cx);
}

fn element_center(window: &Window, id: &str) -> Point<Pixels> {
    let element_id = ElementId::Name(SharedString::from(id));
    gpui_base::test_support::find(window, &[], &element_id)
        .unwrap_or_else(|| panic!("element {id} not found in the last frame"))
        .bounds()
        .center()
}

/// 在元素中心按下-抬起 `count` 次（2 次即双击）。
fn click_element(window: &mut Window, id: &str, count: usize, cx: &mut App) {
    render_frame(window, cx);
    let position = element_center(window, id);
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Default::default(),
        }
        .to_platform_input(),
        cx,
    );
    for click_count in 1..=count {
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count,
            }
            .to_platform_input(),
            cx,
        );
    }
    render_frame(window, cx);
}

fn has_element(window: &Window, id: &str) -> bool {
    let element_id = ElementId::Name(SharedString::from(id));
    gpui_base::test_support::find(window, &[], &element_id).is_some()
}

fn open_host_list(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<HostList>,
    Rc<RefCell<Vec<String>>>,
) {
    cx.update(|cx| gpui_kit::init(cx));
    let host_list: Rc<RefCell<Option<Entity<HostList>>>> = Rc::new(RefCell::new(None));
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let handle = cx.open_window(size(px(800.), px(600.)), |window, cx| {
        let list = cx.new(HostList::new);
        list.update(cx, |list, cx| {
            list.apply_hosts(
                vec![host("local", "本机", true), host("vps1", "vps-1", false)],
                cx,
            );
        });
        *host_list.borrow_mut() = Some(list.clone());
        Root::new(list, window, cx)
    });
    let list = host_list.borrow().clone().unwrap();
    let sink = seen.clone();
    cx.update(|cx| {
        cx.subscribe(&list, move |_, event: &HostListEvent, _| {
            if let HostListEvent::Open(host) = event {
                sink.borrow_mut().push(host.id.clone());
            }
        })
        .detach();
    });
    (handle, list, seen)
}

#[gpui_kit::test]
fn hosts_render_with_their_rows(cx: &mut TestAppContext) {
    let (handle, _, _) = open_host_list(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        render_frame(window, cx);
        assert!(has_element(window, "host-local"));
        assert!(has_element(window, "host-vps1"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn double_click_opens_the_host(cx: &mut TestAppContext) {
    let (handle, _, opened) = open_host_list(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        click_element(window, "host-vps1", 2, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(&*opened.borrow(), &["vps1".to_string()]);
}

#[gpui_kit::test]
fn clicking_an_already_selected_host_opens_it(cx: &mut TestAppContext) {
    let (handle, _, opened) = open_host_list(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        // 第一次点击只选中；再点一次已选中的行即打开（与旧前端一致）。
        click_element(window, "host-local", 1, cx);
        click_element(window, "host-local", 1, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(&*opened.borrow(), &["local".to_string()]);
}
