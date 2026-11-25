//! Puts the program inside the setup, compressed, and gives the setup its icon.

use std::path::Path;

const FILES: [&str; 2] = ["kuro.exe", "THIRD-PARTY-LICENSES.html"];

#[cfg(windows)]
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    println!("cargo::rerun-if-env-changed=KURO_PAYLOAD");
    println!("cargo::rerun-if-changed=../app/assets/app.ico");
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let from = std::env::var("KURO_PAYLOAD").ok();
    for name in FILES {
        // Empty when there is no program: checks and tests do not need one.
        let packed = match &from {
            Some(dir) => {
                let path = Path::new(dir).join(name);
                println!("cargo::rerun-if-changed={}", path.display());
                let bytes = std::fs::read(&path).unwrap_or_else(|e| fail(&path, e));
                miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 9)
            }
            None => Vec::new(),
        };
        let to = Path::new(&out).join(format!("{name}.z"));
        std::fs::write(&to, packed).unwrap_or_else(|e| fail(&to, e));
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../app/assets/app.ico")
            .set("ProductName", "Kuro")
            .set("FileDescription", "Kuro Setup")
            .set("CompanyName", "Simdrom")
            // Without it Windows sees "setup" in the name and asks for administrator rights.
            .set_manifest(MANIFEST);
        if let Err(e) = res.compile() {
            println!("cargo::warning=executable icon skipped: {e}");
        }
    }
}

fn fail(path: &Path, e: std::io::Error) -> ! {
    eprintln!("{}: {e}", path.display());
    std::process::exit(1)
}
