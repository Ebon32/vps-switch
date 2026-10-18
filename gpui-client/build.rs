#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("../assets/app.ico");
    res.set("ProductName", "VPS Switch");
    res.set("FileDescription", "VPS Switch GPUI Desktop Client");
    res.set("LegalCopyright", "Copyright (c) 2026");
    if let Err(e) = res.compile() {
        eprintln!("Failed to compile Windows resources: {}", e);
    }
}

#[cfg(not(windows))]
fn main() {}
