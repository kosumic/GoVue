use std::{env, path::PathBuf, process::Command};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    let (sdk, target) = if env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim") {
        ("iphonesimulator", "arm64-apple-ios16.0-simulator")
    } else {
        ("iphoneos", "arm64-apple-ios16.0")
    };
    let sdk_path = output(Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]));
    let swift = output(Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]));
    let toolchain = PathBuf::from(&swift);
    let runtime = toolchain
        .parent()
        .and_then(std::path::Path::parent)
        .expect("Swift toolchain");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output"));
    let inputs = [
        "../ios/DownloadTransport.swift",
        "../ios/NowPlaying.swift",
        "../../modules/govue-core/ios/DownloadMux.swift",
        "../../modules/govue-core/ios/FileExport.swift",
        "../../modules/govue-core/ios/IosDownloadHttp.swift",
        "../../modules/govue-core/ios/BoundedHttp.swift",
        "../../modules/govue-core/ios/NativeHttpSession.swift",
    ];
    let status = Command::new(swift)
        .args([
            "-sdk",
            &sdk_path,
            "-target",
            target,
            "-module-name",
            "GoVueDownloads",
            "-O",
            "-emit-library",
            "-static",
            "-o",
        ])
        .arg(out.join("libgovue_downloads.a"))
        .args(inputs)
        .status()
        .expect("Apple Swift toolchain");
    assert!(status.success(), "Could not compile the iOS media adapters");
    for input in inputs {
        println!("cargo:rerun-if-changed={input}");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={sdk_path}/usr/lib/swift");
    println!(
        "cargo:rustc-link-search=native={}",
        runtime.join("lib/swift").join(sdk).display()
    );
    println!("cargo:rustc-link-lib=static=govue_downloads");
}

fn output(command: &mut Command) -> String {
    let answer = command.output().expect("Xcode command-line tools");
    assert!(answer.status.success(), "Could not locate the iOS SDK");
    String::from_utf8(answer.stdout)
        .expect("Xcode path")
        .trim()
        .to_owned()
}
