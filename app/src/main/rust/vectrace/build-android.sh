#!/bin/sh
# Build libvectrace.so (Rust) untuk semua ABI Hozin Vector.
# Dipanggil task Gradle buildRustVectrace (preBuild) dan workflow CI.
# Pemakaian: build-android.sh <ndk-dir> <out-dir>
#   out-dir/<abi>/libvectrace.so  (abi: arm64-v8a, armeabi-v7a, x86, x86_64)
# NDK 26.x, API 26 (minSdk). Cargo harus ada di PATH (CI: rustup, ~/.cargo/bin).
set -e
NDK_DIR="$1"
OUT_DIR="$2"
if [ -z "$NDK_DIR" ] || [ -z "$OUT_DIR" ]; then
    echo "pemakaian: build-android.sh <ndk-dir> <out-dir>" >&2
    exit 1
fi
TOOLCHAIN_DIR="$NDK_DIR/toolchains/llvm/prebuilt/linux-x86_64/bin"
if [ ! -d "$TOOLCHAIN_DIR" ]; then
    echo "toolchain NDK tidak ketemu di $TOOLCHAIN_DIR" >&2
    exit 1
fi
API=26
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

build_one() {
    TARGET="$1"; ABI="$2"; CLANG_PREFIX="$3"; LINKER_ENV="$4"
    echo "== vectrace: $TARGET ($ABI)"
    CC="$TOOLCHAIN_DIR/${CLANG_PREFIX}${API}-clang"
    AR="$TOOLCHAIN_DIR/llvm-ar"
    mkdir -p "$OUT_DIR/$ABI"
    # Linker + archiver per target (env CARGO_TARGET_*_LINKER dibaca cargo).
    env "CC=$CC" "AR=$AR" "$LINKER_ENV=$CC" \
        cargo build --release --target "$TARGET"
    cp "target/$TARGET/release/libvectrace.so" "$OUT_DIR/$ABI/libvectrace.so"
    echo "   -> $OUT_DIR/$ABI/libvectrace.so ($(wc -c < "$OUT_DIR/$ABI/libvectrace.so") byte)"
}

build_one aarch64-linux-android arm64-v8a aarch64-linux-android CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER
build_one armv7-linux-androideabi armeabi-v7a armv7a-linux-androideabi CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER
build_one i686-linux-android x86 i686-linux-android CARGO_TARGET_I686_LINUX_ANDROID_LINKER
build_one x86_64-linux-android x86_64 x86_64-linux-android CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER
echo "vectrace: semua ABI selesai."
