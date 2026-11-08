fn main() {
    println!("cargo:rerun-if-changed=assets/launcher95.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/launcher95.ico");
    resource.set("ProductName", "Launcher95");
    resource.set(
        "FileDescription",
        "Windows 95-style portable application launcher",
    );
    resource.set("LegalCopyright", "MIT License");
    resource.set("OriginalFilename", "launcher95.exe");
    if let Err(error) = resource.compile() {
        println!("cargo:warning=Could not embed Windows resources: {error}");
    }
}
