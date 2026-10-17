//! 冒烟：连真实守护进程，为一台主机创建终端 pane，把收到的字节流解析成
//! 网格并打印可见文本。用法：`cargo run -p vps-switch-gpui --example terminal_smoke -- <host>`

use std::time::Duration;

use vps_switch_gpui::terminal::{Session, SessionEvent};

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "local".into());
    let session = Session::spawn(host.clone(), 120, 30);
    println!("session spawned for {host}");
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    let mut closed = None;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        while let Some(event) = session.try_recv() {
            match event {
                SessionEvent::Updated => {}
                SessionEvent::Closed(reason) => {
                    closed = Some(reason);
                }
            }
        }
        if closed.is_some() {
            break;
        }
        if let Some(snapshot) = session.snapshot() {
            if snapshot
                .cells
                .iter()
                .any(|cell| cell.c != ' ' && !cell.c.is_control())
            {
                break;
            }
        }
    }
    match closed {
        Some(reason) => println!("CLOSED: {reason}"),
        None => {}
    }
    let Some(snapshot) = session.snapshot() else {
        println!("NO SNAPSHOT");
        return;
    };
    println!(
        "snapshot {}x{}, cursor={:?}, offset={}, history={}",
        snapshot.cols, snapshot.rows, snapshot.cursor, snapshot.display_offset, snapshot.history_size
    );
    for row in 0..snapshot.rows {
        let line: String = snapshot.cells[row * snapshot.cols..(row + 1) * snapshot.cols]
            .iter()
            .map(|cell| if cell.c.is_control() { ' ' } else { cell.c })
            .collect();
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            println!("{row:02} | {trimmed}");
        }
    }
}
