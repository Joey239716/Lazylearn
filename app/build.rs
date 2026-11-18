// Embed the Lazylearn icon in the .exe as resource 1. GPUI loads that same resource
// as the window/taskbar icon on Windows.
fn main() {
    println!("cargo:rerun-if-changed=../assets/logo/lazylearn.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("../assets/logo/lazylearn.ico", "1");
        res.set("ProductName", "Lazylearn");
        res.set("FileDescription", "Lazylearn");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the app icon: {e}");
        }
    }
}
