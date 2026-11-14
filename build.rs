fn main() {
    slint_build::compile("ui/appwindow.slint").expect("Slint build failed");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        let target = std::env::var("TARGET").unwrap_or_default();
        if target.contains("windows-gnu") {
            res.set_windres_path("x86_64-w64-mingw32-windres");
            res.set_ar_path("x86_64-w64-mingw32-ar");
        }
        res.compile().expect("winres compilation failed");
        if target.contains("windows-gnu") {
            let out_dir = std::env::var("OUT_DIR").unwrap();
            println!("cargo:rustc-link-arg={}/resource.o", out_dir);
        }
    }
}
