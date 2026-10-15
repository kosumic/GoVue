Pod::Spec.new do |s|
  s.name = 'LingoPlayCore'
  s.version = '0.1.0'
  s.summary = 'Shared Rust transcript core for LingoPlay'
  s.description = s.summary
  s.author = 'LingoPlay'
  s.homepage = 'https://github.com/kaminarios/lingoplay'
  s.license = { :type => 'UNLICENSED' }
  s.platforms = { :ios => '16.4' }
  s.source = { :git => '' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = '**/*.{swift,h}'
  s.exclude_files = 'LingoPlayRust.xcframework/**/*'
  s.vendored_frameworks = 'LingoPlayRust.xcframework'
  s.preserve_paths = 'generated/*'
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_INCLUDE_PATHS' => '$(PODS_TARGET_SRCROOT)/generated',
    'OTHER_SWIFT_FLAGS' => '$(inherited) -Xcc -fmodule-map-file="$(PODS_TARGET_SRCROOT)/generated/LingoPlayRustFFI.modulemap"'
  }
  s.swift_version = '5.9'
end
