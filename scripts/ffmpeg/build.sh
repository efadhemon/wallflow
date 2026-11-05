#!/usr/bin/env bash
# Builds the trimmed, static, LGPL-only FFmpeg that Wallflow bundles on
# Windows and macOS: just what the live wallpaper engine runs (decode to raw
# NV12, `prepare`'s scale / fps / H.264 encode, the JPEG poster, ffprobe).
#
#   scripts/ffmpeg/build.sh <target-triple> <out-dir>
#
# Targets: x86_64-pc-windows-msvc (cross-built with mingw-w64 on Linux),
# aarch64-apple-darwin / x86_64-apple-darwin (on macOS), x86_64-unknown-linux-gnu
# (local check of the flags only; Linux uses the distribution's FFmpeg).
# WORK=dir keeps the source and objects between runs.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=version.env
source "$here/version.env"
target=${1:?target triple}
out=$(mkdir -p "${2:?out dir}" && cd "$2" && pwd)
work=${WORK:-$PWD/target/ffmpeg-$target}
src=$work/src
build=$work/build
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)

if [ ! -d "$src/.git" ]; then
  git clone --quiet --depth 1 --branch "$FFMPEG_TAG" https://github.com/FFmpeg/FFmpeg.git "$src"
fi
actual=$(git -C "$src" rev-parse HEAD)
if [ "$actual" != "$FFMPEG_COMMIT" ]; then
  echo "error: $FFMPEG_TAG is $actual, expected $FFMPEG_COMMIT" >&2
  exit 1
fi

flags=(
  --disable-everything --disable-autodetect
  --disable-gpl --disable-nonfree --disable-version3
  --enable-static --disable-shared
  --disable-doc --disable-debug --disable-ffplay --disable-network --disable-avdevice
  --enable-ffmpeg --enable-ffprobe
  --enable-protocol=file,pipe
  --enable-demuxer=mov,matroska,avi,mpegts
  --enable-muxer=mp4,rawvideo,image2
  # AV1 decodes only with a hardware decoder here (no libdav1d yet).
  --enable-decoder=h264,hevc,vp8,vp9,av1,mpeg4,mpeg2video,prores,mjpeg
  --enable-parser=h264,hevc,vp8,vp9,av1,mpeg4video,mpegvideo,mjpeg
  --enable-encoder=rawvideo,mjpeg
  --enable-filter=buffer,buffersink,null,format,scale,fps,setsar
)
exe=

case "$target" in
  x86_64-pc-windows-msvc)
    exe=.exe
    flags+=(
      --target-os=mingw32 --arch=x86_64 --cross-prefix=x86_64-w64-mingw32-
      --enable-w32threads --enable-d3d11va --enable-dxva2 --enable-mediafoundation
      --enable-encoder=h264_mf
      --enable-hwaccel=h264_d3d11va,h264_d3d11va2,hevc_d3d11va,hevc_d3d11va2,vp9_d3d11va,vp9_d3d11va2,av1_d3d11va,av1_d3d11va2,h264_dxva2,hevc_dxva2,vp9_dxva2
      # No libwinpthread / libgcc DLLs next to the executable.
      --extra-ldflags=-static
    )
    ;;
  aarch64-apple-darwin | x86_64-apple-darwin)
    arch=${target%%-*}
    [ "$arch" = aarch64 ] && arch=arm64
    flags+=(
      --enable-videotoolbox --enable-encoder=h264_videotoolbox
      --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,vp9_videotoolbox,av1_videotoolbox,prores_videotoolbox
      --arch="$arch" --cc="clang -arch $arch"
      --extra-cflags=-mmacosx-version-min=11.0 --extra-ldflags=-mmacosx-version-min=11.0
    )
    [ "$arch" != "$(uname -m)" ] && flags+=(--enable-cross-compile)
    ;;
  x86_64-unknown-linux-gnu)
    command -v nasm >/dev/null || flags+=(--disable-x86asm)
    ;;
  *)
    echo "error: unsupported target $target" >&2
    exit 1
    ;;
esac

mkdir -p "$build"
cd "$build"
"$src/configure" "${flags[@]}" >configure.log || { tail -30 configure.log >&2; exit 1; }
# The license is the point of this build: refuse anything but LGPL.
grep -q '^#define CONFIG_GPL 0' config.h
grep -q '^#define CONFIG_NONFREE 0' config.h
make -j"$jobs" >make.log 2>&1 || { tail -40 make.log >&2; exit 1; }

cp "ffmpeg$exe" "ffprobe$exe" "$out/"
cp "$src/COPYING.LGPLv2.1" "$out/FFmpeg-LICENSE.txt"
{
  echo "FFmpeg $FFMPEG_TAG ($FFMPEG_COMMIT), Wallflow build $WALLFLOW_FFMPEG_BUILD, $target"
  echo "Source: https://github.com/FFmpeg/FFmpeg/tree/$FFMPEG_COMMIT"
  echo "License: LGPL v2.1 or later"
  echo "configure ${flags[*]}"
} >"$out/FFmpeg-BUILD.txt"
ls -l "$out"
