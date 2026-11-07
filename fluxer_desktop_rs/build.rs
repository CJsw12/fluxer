// SPDX-License-Identifier: AGPL-3.0-or-later

fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    println!("cargo:rerun-if-changed=resources/tray_32x32.png");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("resources/icon.ico");
        res.set("ProductName", "Fluxer");
        res.set("FileDescription", "Fluxer");
        res.set("InternalName", "Fluxer");
        res.set("OriginalFilename", "Fluxer.exe");
        res.set("LegalCopyright", "AGPL-3.0-or-later");
        if let Err(err) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resource: {err}");
        }
    }
}
