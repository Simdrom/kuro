//! Gives the Windows executable its icon.

fn main() {
    println!("cargo::rerun-if-changed=assets/app.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico").set("ProductName", "Mango Desktop");
        if let Err(e) = res.compile() {
            // Without the Windows SDK the program still builds, only without an icon.
            println!("cargo::warning=executable icon skipped: {e}");
        }
    }
}
