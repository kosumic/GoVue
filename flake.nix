{
  description = "GoVue mobile development environment";
  inputs.nixpkgs.url = "tarball+https://releases.nixos.org/nixos/unstable/nixos-26.11pre1034379.18b9261cb329/nixexprs.tar.xz";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { nixpkgs, rust-overlay, ... }: let
    systems = [ "x86_64-linux" "aarch64-darwin" ];
    rustVersion = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml)).toolchain.channel;
  in {
    devShells = nixpkgs.lib.genAttrs systems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
        config.allowUnfree = true;
        config.android_sdk.accept_license = true;
      };
      androidSdk = (pkgs.androidenv.composeAndroidPackages {
        platformVersions = [ "36" ];
        buildToolsVersions = [ "35.0.0" "36.0.0" ];
        cmakeVersions = [ "3.22.1" ];
        includeNDK = true;
        ndkVersions = [ "27.1.12297006" ];
      }).androidsdk;
      androidNdkTools = "${androidSdk}/libexec/android-sdk/ndk-bundle/toolchains/llvm/prebuilt/linux-x86_64/bin";
    in {
      default = pkgs.mkShell ({
        packages = [ pkgs.nodejs_24 pkgs.libwebp pkgs.resvg (pkgs.rust-bin.stable.${rustVersion}.default.override {
          targets = if pkgs.stdenv.isLinux then [ "aarch64-linux-android" ] else [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
        }) ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ androidSdk pkgs.jdk17_headless ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.watchman
            (pkgs.ruby_3_4.withPackages (gems: [ gems.xcodeproj ]))
            pkgs.bundler
            pkgs.cocoapods
            pkgs.fastlane
          ];
      } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
        ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
        ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
        ANDROID_NDK_ROOT = "${androidSdk}/libexec/android-sdk/ndk-bundle";
        JAVA_HOME = pkgs.jdk17_headless.home;
        GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/36.0.0/aapt2";
        ORG_GRADLE_PROJECT_reactNativeArchitectures = "arm64-v8a";
        CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "${androidNdkTools}/aarch64-linux-android24-clang";
        CC_aarch64_linux_android = "${androidNdkTools}/aarch64-linux-android24-clang";
        AR_aarch64_linux_android = "${androidNdkTools}/llvm-ar";
        CFLAGS_aarch64_linux_android = "--target=aarch64-none-linux-android24";
      } // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
        shellHook = ''
          export DEVELOPER_DIR="$(env -u DEVELOPER_DIR -u SDKROOT /usr/bin/xcode-select -p)"
          unset SDKROOT CC CXX LD AR AS RANLIB STRIP
          export PATH="/usr/bin:/bin:$PATH"
        '';
      });
    });
  };
}
