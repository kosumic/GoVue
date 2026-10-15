{
  description = "LingoPlay mobile development environment";
  inputs.nixpkgs.url = "tarball+https://releases.nixos.org/nixos/unstable/nixos-26.11pre1034379.18b9261cb329/nixexprs.tar.xz";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { nixpkgs, rust-overlay, ... }: let
    systems = [ "x86_64-linux" "aarch64-darwin" ];
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
    in {
      default = pkgs.mkShell ({
        packages = [ pkgs.nodejs_22 pkgs.cargo-ndk (pkgs.rust-bin.stable."1.90.0".minimal.override {
          extensions = [ "rustfmt" ];
          targets = if pkgs.stdenv.isLinux then [ "aarch64-linux-android" ] else [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
        }) ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ androidSdk pkgs.jdk17_headless ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.watchman pkgs.cocoapods ];
      } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
        ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
        ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
        JAVA_HOME = pkgs.jdk17_headless.home;
        GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/36.0.0/aapt2";
        ORG_GRADLE_PROJECT_reactNativeArchitectures = "arm64-v8a";
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
