fn main() {
    // Windows gives the main thread 1 MiB of stack. The Slint compiler
    // recurses over the element tree and blows past that on a UI of any
    // depth, so do the work on a thread we can size ourselves.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(compile)
        .expect("failed to spawn the Slint build thread")
        .join()
        .expect("the Slint build thread panicked");
}

fn compile() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("failed to compile ui/app.slint");
    embed_icon();
}

/// Puts the icon into the executable's Windows resources, which is what
/// Explorer, the taskbar and the Start Menu shortcut all read.
#[cfg(windows)]
fn embed_icon() {
    println!("cargo:rerun-if-changed=assets/mangix.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/mangix.ico");
    resource.set("ProductName", "Mangix");
    resource.set("FileDescription", "Mangix comic reader");
    if let Err(e) = resource.compile() {
        println!("cargo:warning=could not embed the icon: {e}");
    }
}

#[cfg(not(windows))]
fn embed_icon() {}
