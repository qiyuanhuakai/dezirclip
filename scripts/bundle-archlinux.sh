#!/usr/bin/env bash
set -euo pipefail

# Run under the non-root build user in Arch Linux's base-devel image.
tag=$1
tauri_deb=$2
mygo_deb=$3
output=$4
if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)(-([a-z]+)\.?([0-9]+))?$ ]]; then
    echo "Invalid Arch package release tag: $tag" >&2
    exit 1
fi
version=${tag#v}
pkgver=${BASH_REMATCH[1]}${BASH_REMATCH[3]}${BASH_REMATCH[4]}
repo=https://github.com/qiyuanhuakai/dezirclip
export HOME=/tmp PKGEXT=.pkg.tar.zst SRCEXT=.src.tar.gz
mkdir -p "$output"
output=$(realpath "$output")
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

build_package() (
    local name=$1 label=$2 prefix=$3 deb=$4 desktop=$5 binary=$6
    shift 6
    local asset="${prefix}_${version}_amd64.deb"
    [[ $(basename "$deb") == "$asset" && -s "$deb" ]]
    local checksum
    checksum=$(sha256sum "$deb")
    checksum=${checksum%% *}
    mkdir "$work/$name"
    cp -- "$deb" "$work/$name/$asset"
    cd "$work/$name"

    {
        cat <<EOF
pkgname='$name'
pkgver='$pkgver'
pkgrel=1
pkgdesc='DezirClip clipboard manager ($label)'
arch=('x86_64')
url='$repo'
license=('GPL-3.0-or-later')
provides=('${name%-bin}=$pkgver')
conflicts=('${name%-bin}')
EOF
        printf 'depends=('
        printf "'%s' " "$@"
        printf ')\n'
        cat <<EOF
options=('!strip' '!debug' '!zipman' '!purge')
source=('$repo/releases/download/$tag/$asset')
noextract=('$asset')
sha256sums=('$checksum')
EOF
        cat <<'EOF'

package() {
    local deb="$srcdir/${source[0]##*/}" data
    data=$(bsdtar -tf "$deb" | grep -E '^data\.tar(\.(gz|xz|zst|bz2))?$') || return
    bsdtar -xOf "$deb" "$data" | bsdtar -xpf - -C "$pkgdir" usr
}
EOF
    } > PKGBUILD
    export PKGDEST="$output" SRCPKGDEST="$output"
    makepkg --nodeps --nosign --nocolor
    makepkg --printsrcinfo > .SRCINFO
    makepkg --source --nosign --nocolor

    local package="$output/$name-$pkgver-1-x86_64.pkg.tar.zst"
    local recipe="$output/$name-$pkgver-1.src.tar.gz"
    pacman -Qp -- "$package" | grep -Fx "$name $pkgver-1"
    local paths
    paths=$(bsdtar -tf "$package")
    grep -Fxq "$desktop" <<< "$paths"
    grep -Fxq "$binary" <<< "$paths"
    grep -Fxq "$name/.SRCINFO" < <(bsdtar -tf "$recipe")
    grep -Fxq "$name/PKGBUILD" < <(bsdtar -tf "$recipe")
    grep -Fxq 'arch = x86_64' < <(bsdtar -xOf "$package" .PKGINFO)
)

build_package dezirclip-bin Tauri DezirClip "$tauri_deb"     usr/share/applications/DezirClip.desktop usr/bin/dezirclip     gtk3 webkit2gtk-4.1 libayatana-appindicator xdg-utils libx11 libxi libxrandr libxtst
build_package dezirclip-mygo-bin 'MyGo Native' DezirClip-MyGo "$mygo_deb"     usr/share/applications/DezirClip-MyGo.desktop usr/bin/dezirclip-mygo     gtk3 webkit2gtk-4.1 gst-plugins-base gst-plugins-good xdg-utils libx11 libxi libxrandr libxtst dbus
