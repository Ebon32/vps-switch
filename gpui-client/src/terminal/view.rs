//! 终端视图：GPUI Element 渲染网格快照 + 键盘输入编码。
//!
//! 渲染策略（性能与正确性的折中，依据 `force_width` 的真实语义）：
//! `shape_line` 的 `force_width` 会把**每个字形**吸附到 `n × 单格宽度` 的
//! 网格上。对 ASCII 等宽字符这是精确对齐；对中文等双宽字符会把后续字符
//! 挤进上一格。因此每行按段塑形：连续 ASCII 段带 `force_width` 合并成一次
//! 塑形，双宽字符单独塑形、画在 `列号 × cell_width` 处。

use std::time::Duration;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use gpui_kit::component::*;
use gpui_kit::*;

use super::session::{Session, SessionCommand, SessionEvent, Snapshot};

const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 1.4;

pub struct TerminalView {
    session: Session,
    snapshot: Option<Snapshot>,
    focus_handle: FocusHandle,
    /// 会话结束的原因；有值时在底部显示提示条。
    status: Option<String>,
    /// 白底 / 黑底（与旧前端的 LIGHT_THEME / DARK_THEME 对应）。
    dark: bool,
    cols: u16,
    rows: u16,
}

impl TerminalView {
    pub fn new(host_id: String, dark: bool, cx: &mut Context<Self>) -> Self {
        let (cols, rows) = (100, 30);
        let this = Self {
            session: Session::spawn(host_id, cols, rows),
            snapshot: None,
            focus_handle: cx.focus_handle(),
            status: None,
            dark,
            cols,
            rows,
        };
        Self::poll(cx);
        this
    }

    pub fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
        if self.dark != dark {
            self.dark = dark;
            cx.notify();
        }
    }

    fn poll(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let alive = this.update(cx, |this, cx| {
                let mut changed = false;
                while let Some(event) = this.session.try_recv() {
                    changed = true;
                    match event {
                        SessionEvent::Updated => {
                            this.snapshot = this.session.snapshot();
                        }
                        SessionEvent::Closed(reason) => {
                            this.status = Some(reason);
                        }
                    }
                }
                if changed {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    fn request_size(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 || (cols == self.cols && rows == self.rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.session.send(SessionCommand::Resize { cols, rows });
    }

    fn input(&mut self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.session.send(SessionCommand::Input(bytes));
        }
    }

    fn scroll(&mut self, lines: i32) {
        self.session.send(SessionCommand::Scroll(lines));
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let focus = self.focus_handle.clone();
        let palette = palette(self.dark);
        let mut root = div()
            .size_full()
            .bg(Hsla::from(rgb(palette.background)))
            .track_focus(&focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if let Some(bytes) = encode_key(&event.keystroke) {
                    this.input(bytes);
                    cx.stop_propagation();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, _cx| {
                let dy = event.delta.pixel_delta(px(FONT_SIZE)).y;
                if dy != px(0.) {
                    let lines = (dy.as_f32() / (FONT_SIZE * LINE_HEIGHT)).round() as i32;
                    if lines != 0 {
                        this.scroll(lines);
                    }
                }
            }))
            .child(TerminalElement { view: this });
        if let Some(status) = &self.status {
            root = root.child(
                div()
                    .absolute()
                    .bottom_2()
                    .left_1_2()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().danger)
                    .text_sm()
                    .text_color(cx.theme().danger_foreground)
                    .child(status.clone()),
            );
        }
        root
    }
}

struct TerminalElement {
    view: Entity<TerminalView>,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct TerminalPaintState {
    segments: Vec<Segment>,
    backgrounds: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    cell_width: Pixels,
    line_height: Pixels,
}

struct Segment {
    origin: Point<Pixels>,
    line: ShapedLine,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = TerminalPaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let font_size = px(FONT_SIZE);
        let mono_family = cx.theme().mono_font_family.clone();
        let mono = font(mono_family);
        let measure = TextRun {
            len: 1,
            font: mono.clone(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let cell_width = window
            .text_system()
            .shape_line("M".into(), font_size, &[measure], None)
            .width()
            .max(px(1.));
        let line_height = window.pixel_snap(font_size * LINE_HEIGHT).max(px(1.));
        let cols = ((bounds.size.width / cell_width).floor() as usize).max(1);
        let rows = ((bounds.size.height / line_height).floor() as usize).max(1);

        let snapshot = self.view.read(cx).snapshot.clone();
        let mut state = TerminalPaintState {
            segments: Vec::new(),
            backgrounds: Vec::new(),
            cursor: None,
            cell_width,
            line_height,
        };
        let Some(snapshot) = snapshot else {
            return state;
        };

        // 尺寸变化同步给后端 PTY。
        let clamped_cols = cols.min(u16::MAX as usize) as u16;
        let clamped_rows = rows.min(u16::MAX as usize) as u16;
        if clamped_cols != snapshot.cols as u16 || clamped_rows != snapshot.rows as u16 {
            self.view
                .update(cx, |view, _| view.request_size(clamped_cols, clamped_rows));
        }

        let colors = palette(self.view.read(cx).dark);
        let default_fg: Hsla = Hsla::from(rgb(colors.foreground));
        let default_bg: Hsla = Hsla::from(rgb(colors.background));
        let palette: [Hsla; 16] = colors.ansi.map(|hex| Hsla::from(rgb(hex)));

        let rows_to_draw = rows.min(snapshot.rows);
        let cols_to_draw = cols.min(snapshot.cols);
        for row in 0..rows_to_draw {
            let y = bounds.top() + line_height * row as f32;
            let mut col = 0;
            // 背景：连续的非默认背景区间合并成矩形。
            while col < cols_to_draw {
                let cell = &snapshot.cells[row * snapshot.cols + col];
                if is_default_bg(cell.bg) {
                    col += 1;
                    continue;
                }
                let color = resolve_color(cell.bg, default_fg, default_bg, &palette);
                let start = col;
                while col < cols_to_draw {
                    let next = &snapshot.cells[row * snapshot.cols + col];
                    if is_default_bg(next.bg)
                        || resolve_color(next.bg, default_fg, default_bg, &palette) != color
                    {
                        break;
                    }
                    col += 1;
                }
                let rect = Bounds {
                    origin: point(bounds.left() + cell_width * start as f32, y),
                    size: size(cell_width * (col - start) as f32, line_height),
                };
                state.backgrounds.push(fill(rect, color));
            }

            // 文本：按段塑形。
            col = 0;
            while col < cols_to_draw {
                let cell = &snapshot.cells[row * snapshot.cols + col];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) || cell.c == ' ' {
                    col += 1;
                    continue;
                }
                let wide = cell.flags.contains(Flags::WIDE_CHAR);
                let start_col = col;
                if wide {
                    // 双宽字符单独塑形，按列定位；不经过 force_width 网格。
                    let text = cell.c.to_string();
                    let run = TextRun {
                        len: text.len(),
                        font: styled_font(&mono, cell.flags),
                        color: resolve_color(cell.fg, default_fg, default_bg, &palette),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window
                        .text_system()
                        .shape_line(SharedString::from(text), font_size, &[run], None);
                    state.segments.push(Segment {
                        origin: point(bounds.left() + cell_width * start_col as f32, y),
                        line,
                    });
                    col += 1;
                    continue;
                }
                // 连续单宽字符段（同前景色）合并为一次塑形。
                let fg = resolve_color(cell.fg, default_fg, default_bg, &palette);
                let mut text = String::new();
                let mut runs: Vec<TextRun> = Vec::new();
                while col < cols_to_draw {
                    let next = &snapshot.cells[row * snapshot.cols + col];
                    if next.flags.contains(Flags::WIDE_CHAR_SPACER)
                        || next.flags.contains(Flags::WIDE_CHAR)
                    {
                        break;
                    }
                    let next_fg = resolve_color(next.fg, default_fg, default_bg, &palette);
                    if next_fg != fg && !text.is_empty() {
                        break;
                    }
                    let ch = next.c;
                    let len = ch.len_utf8();
                    text.push(ch);
                    match runs.last_mut() {
                        Some(run) => run.len += len,
                        None => runs.push(TextRun {
                            len,
                            font: mono.clone(),
                            color: fg,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }),
                    }
                    col += 1;
                }
                if text.trim().is_empty() {
                    // 纯空白不塑形，只推进列（背景已经画过）。
                    continue;
                }
                let line = window.text_system().shape_line(
                    SharedString::from(text),
                    font_size,
                    &runs,
                    Some(cell_width),
                );
                state.segments.push(Segment {
                    origin: point(bounds.left() + cell_width * start_col as f32, y),
                    line,
                });
            }
        }

        // 光标：实心块。
        if let Some((row, col)) = snapshot.cursor.filter(|(row, _)| *row < rows_to_draw) {
            let rect = Bounds {
                origin: point(
                    bounds.left() + cell_width * col as f32,
                    bounds.top() + line_height * row as f32,
                ),
                size: size(cell_width, line_height),
            };
            let color = match snapshot.cursor_shape {
                CursorShape::Hidden => {
                    state.cursor = None;
                    return state;
                }
                CursorShape::Block | CursorShape::HollowBlock | CursorShape::Beam => {
                    Hsla::from(rgb(colors.cursor)).opacity(0.7)
                }
                CursorShape::Underline => Hsla::from(rgb(colors.cursor)).opacity(0.5),
            };
            state.cursor = Some(fill(rect, color));
        }

        state
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = ContentMask { bounds };
        window.with_content_mask(Some(mask), |window| {
            for quad in state.backgrounds.drain(..) {
                window.paint_quad(quad);
            }
            for segment in &state.segments {
                let _ = segment.line.paint(
                    segment.origin,
                    state.line_height,
                    TextAlign::Left,
                    Some(state.cell_width * 4.),
                    window,
                    cx,
                );
            }
            if let Some(cursor) = state.cursor.take() {
                window.paint_quad(cursor);
            }
        });
    }
}

/// 终端配色，与旧前端 `TerminalPaneView.vue` 的 LIGHT_THEME / DARK_THEME 对齐。
struct Palette {
    background: u32,
    foreground: u32,
    cursor: u32,
    ansi: [u32; 16],
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: 0x0c0d10,
            foreground: 0xe4e4e7,
            cursor: 0xf4f4f5,
            ansi: [
                0x27272a, 0xf87171, 0x4ade80, 0xfacc15, 0x60a5fa, 0xc084fc, 0x22d3ee,
                0xe4e4e7, 0x71717a, 0xfca5a5, 0x86efac, 0xfde047, 0x93c5fd, 0xd8b4fe,
                0x67e8f9, 0xffffff,
            ],
        }
    } else {
        Palette {
            background: 0xffffff,
            foreground: 0x18181b,
            cursor: 0x18181b,
            ansi: [
                0x18181b, 0xdc2626, 0x16a34a, 0xca8a04, 0x2563eb, 0x9333ea, 0x0891b2,
                0xe4e4e7, 0x71717a, 0xef4444, 0x22c55e, 0xeab308, 0x3b82f6, 0xa855f7,
                0x06b6d4, 0xfafafa,
            ],
        }
    }
}

fn is_default_bg(color: Color) -> bool {
    matches!(color, Color::Named(NamedColor::Background))
}

fn resolve_color(color: Color, default_fg: Hsla, default_bg: Hsla, palette: &[Hsla; 16]) -> Hsla {
    match color {
        Color::Named(named) => match named {
            NamedColor::Foreground | NamedColor::BrightForeground => default_fg,
            NamedColor::Background => default_bg,
            NamedColor::DimForeground => default_fg.opacity(0.66),
            NamedColor::Cursor => default_fg,
            other => named_index(other)
                .map(|index| palette[index])
                .unwrap_or(default_fg),
        },
        Color::Indexed(index) => palette[index as usize % palette.len()],
        Color::Spec(rgb_color) => Hsla::from(rgb(u32::from(rgb_color.r) << 16 | u32::from(rgb_color.g) << 8 | u32::from(rgb_color.b))),
    }
}

fn named_index(color: NamedColor) -> Option<usize> {
    use NamedColor::*;
    Some(match color {
        Black | DimBlack => 0,
        Red | DimRed => 1,
        Green | DimGreen => 2,
        Yellow | DimYellow => 3,
        Blue | DimBlue => 4,
        Magenta | DimMagenta => 5,
        Cyan | DimCyan => 6,
        White | DimWhite => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        _ => return None,
    })
}

fn styled_font(font: &Font, flags: Flags) -> Font {
    let mut font = font.clone();
    if flags.contains(Flags::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        font.style = FontStyle::Italic;
    }
    font
}

/// 把一次按键编码成终端字节。M2 第一版覆盖 ASCII 与控制键；
/// IME / 中文输入走 InputHandler 路径（后续里程碑）。
fn encode_key(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let key = keystroke.key.as_str();
    let ctrl = keystroke.modifiers.control;
    let alt = keystroke.modifiers.alt;
    let sequence: Vec<u8> = match key {
        "enter" => b"\r".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "space" if ctrl => vec![0x00],
        _ => {
            if ctrl {
                let ch = key.chars().next()?.to_ascii_lowercase();
                if ch.is_ascii_lowercase() {
                    vec![(ch as u8) & 0x1f]
                } else if key == "[" {
                    vec![0x1b]
                } else if key == "\\" {
                    vec![0x1c]
                } else if key == "]" {
                    vec![0x1d]
                } else {
                    return None;
                }
            } else {
                let text = keystroke.key_char.clone()?;
                text.into_bytes()
            }
        }
    };
    if alt {
        let mut bytes = vec![0x1b];
        bytes.extend_from_slice(&sequence);
        Some(bytes)
    } else {
        Some(sequence)
    }
}
