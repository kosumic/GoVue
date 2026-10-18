Pod::Spec.new do |s|
  s.name = 'GoVueCore'
  s.version = '0.1.0'
  s.summary = 'Shared Rust player, browser and media core for GoVue'
  s.description = s.summary
  s.author = 'GoVue'
  s.homepage = 'https://github.com/kosumic/GoVue'
  s.license = { :type => 'GPL-3.0-only' }
  s.platforms = { :ios => '16.4' }
  s.source = { :git => '' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = '**/*.{swift,h}'
  s.exclude_files = 'GoVueRust.xcframework/**/*'
  s.vendored_frameworks = 'GoVueRust.xcframework'
  s.preserve_paths = 'generated/*'
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_INCLUDE_PATHS' => '$(PODS_TARGET_SRCROOT)/generated',
    'OTHER_SWIFT_FLAGS' => '$(inherited) -Xcc -fmodule-map-file="$(PODS_TARGET_SRCROOT)/generated/GoVueRustFFI.modulemap"'
  }
  s.swift_version = '5.9'
end
