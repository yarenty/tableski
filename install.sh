#!/usr/bin/env sh
# tableski installer: downloads the latest release binary for this OS/arch.
#   curl -fsSL https://raw.githubusercontent.com/yarenty/tableski/main/install.sh | sh
set -eu

REPO="yarenty/tableski"
BIN_DIR="${TABLESKI_BIN_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)  target="x86_64-unknown-linux-gnu" ;;
  Darwin)
    case "$arch" in
      arm64) target="aarch64-apple-darwin" ;;
      *)     target="x86_64-apple-darwin" ;;
    esac ;;
  *) echo "unsupported OS: $os (Windows: download the .zip from the releases page)"; exit 1 ;;
esac

tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep -m1 '"tag_name"' | cut -d'"' -f4)
[ -n "$tag" ] || { echo "cannot determine latest release"; exit 1; }

url="https://github.com/$REPO/releases/download/$tag/tableski-$tag-$target.tar.gz"
echo "installing tableski $tag ($target) -> $BIN_DIR"
mkdir -p "$BIN_DIR"
curl -fsSL "$url" | tar xz -C "$BIN_DIR"
chmod +x "$BIN_DIR/tableski"
echo "done: $("$BIN_DIR/tableski" --help >/dev/null 2>&1 && echo OK) — ensure $BIN_DIR is on your PATH"
