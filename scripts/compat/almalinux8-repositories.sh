# Keep repository metadata and packages on the same HTTPS origin during a mirror sync.
almalinux_dnf() {
    dnf --refresh \
        --setopt='baseos.mirrorlist=' \
        --setopt='appstream.mirrorlist=' \
        --setopt='extras.mirrorlist=' \
        --setopt='baseos.baseurl=https://repo.almalinux.org/almalinux/$releasever/BaseOS/$basearch/os/' \
        --setopt='appstream.baseurl=https://repo.almalinux.org/almalinux/$releasever/AppStream/$basearch/os/' \
        --setopt='extras.baseurl=https://repo.almalinux.org/almalinux/$releasever/extras/$basearch/os/' \
        "$@"
}
