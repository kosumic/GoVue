#[cfg(target_os = "ios")]
fn main() {
    nsh::run_ios();
}

#[cfg(not(target_os = "ios"))]
fn main() {}
