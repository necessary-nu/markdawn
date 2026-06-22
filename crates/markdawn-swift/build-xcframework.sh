#!/usr/bin/env bash
# Build markdawn-swift + every markdawn-swift-lang-* plugin for the three
# iOS-flavored slices we ship to Swift consumers. Output: one main XCFramework
# (Markdawn.xcframework, static library) and one XCFramework per language
# plugin (MarkdawnLang{Name}.xcframework, dynamic framework).
#
# Set LANGUAGES="rust json ..." to build a subset for incremental work
# (otherwise builds every markdawn-swift-lang-* crate found).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"

rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-ios-macabi

# TextKit 2 (NSTextContentStorage) is iOS 15+.
export IPHONEOS_DEPLOYMENT_TARGET=15.0

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-ios-macabi)
SLICE_DIRS=(ios-arm64 ios-arm64-simulator ios-arm64-maccatalyst)

# ---------- Main framework (static library) ----------

echo "==> building main framework"
for target in "${TARGETS[@]}"; do
  cargo build --release --target "$target" -p markdawn-swift \
    --manifest-path "$repo_root/Cargo.toml"
done

main_out="$here/Markdawn.xcframework"
rm -rf "$main_out"
xcodebuild -create-xcframework \
  -library "$repo_root/target/aarch64-apple-ios/release/libmarkdawn_swift.a"        -headers "$here/include" \
  -library "$repo_root/target/aarch64-apple-ios-sim/release/libmarkdawn_swift.a"    -headers "$here/include" \
  -library "$repo_root/target/aarch64-apple-ios-macabi/release/libmarkdawn_swift.a" -headers "$here/include" \
  -output "$main_out" >/dev/null

# ---------- Language plugins (dynamic frameworks) ----------

# Resolve the plugin list. LANGUAGES env-var wins; otherwise enumerate
# everything we generated under crates/markdawn-swift-lang-*.
plugins=()
if [[ -n "${LANGUAGES:-}" ]]; then
  for name in $LANGUAGES; do
    plugins+=("$name")
  done
else
  for d in "$repo_root"/crates/markdawn-swift-lang-*; do
    [[ -d "$d" ]] || continue
    plugins+=("$(basename "$d" | sed 's/^markdawn-swift-lang-//')")
  done
fi
echo "==> building ${#plugins[@]} language plugins: ${plugins[*]}"

# Single cargo build per target with -p for each plugin — cargo parallelizes.
for target in "${TARGETS[@]}"; do
  pkg_args=()
  for name in "${plugins[@]}"; do
    pkg_args+=(-p "markdawn-swift-lang-$name")
  done
  cargo build --release --target "$target" "${pkg_args[@]}" \
    --manifest-path "$repo_root/Cargo.toml"
done

# Convert "rust" -> "Rust", "c-sharp" -> "CSharp", "ssh-config" -> "SshConfig"
pascal_case() {
  echo "$1" | awk -F'-' '{for(i=1;i<=NF;i++) $i = toupper(substr($i,1,1)) substr($i,2)} 1' OFS=''
}

# Convert "rust" -> "rust", "c-sharp" -> "c_sharp"
sanitize_symbol() {
  echo "$1" | tr '-' '_'
}

build_root="$here/build"
rm -rf "$build_root"
mkdir -p "$build_root"

for name in "${plugins[@]}"; do
  pascal="$(pascal_case "$name")"
  symbol="$(sanitize_symbol "$name")"
  fw_name="MarkdawnLang$pascal"
  dylib_name="libmarkdawn_swift_lang_${symbol}.dylib"

  framework_args=()
  for i in 0 1 2; do
    target="${TARGETS[$i]}"
    slice_dir="${SLICE_DIRS[$i]}"
    src="$repo_root/target/$target/release/$dylib_name"
    fw_dir="$build_root/$slice_dir/$fw_name.framework"
    mkdir -p "$fw_dir"
    cp "$src" "$fw_dir/$fw_name"

    # Fix install name so dlopen via @rpath/$fw_name.framework/$fw_name works.
    install_name_tool -id "@rpath/$fw_name.framework/$fw_name" "$fw_dir/$fw_name"

    # Minimal Info.plist. CFBundleExecutable must match the binary name.
    case "$slice_dir" in
      ios-arm64)               platform="iPhoneOS"   ; min_os="15.0" ;;
      ios-arm64-simulator)     platform="iPhoneSimulator"; min_os="15.0" ;;
      ios-arm64-maccatalyst)   platform="MacOSX"     ; min_os="15.0" ;;
    esac
    cat > "$fw_dir/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$fw_name</string>
  <key>CFBundleIdentifier</key><string>nu.necessary.markdawn.$symbol</string>
  <key>CFBundleName</key><string>$fw_name</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>MinimumOSVersion</key><string>$min_os</string>
</dict>
</plist>
PLIST

    framework_args+=(-framework "$fw_dir")
  done

  out="$here/$fw_name.xcframework"
  rm -rf "$out"
  xcodebuild -create-xcframework "${framework_args[@]}" -output "$out" >/dev/null
done

# ---------- Summary ----------

echo
echo "==> XCFramework sizes:"
du -sh "$here"/*.xcframework 2>/dev/null | sort -h
