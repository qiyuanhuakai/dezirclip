FROM almalinux:8
# x11rb's allow-unsafe-code feature passes -lxcb to the linker; unused XCB
# functions are discarded from the final standalone ELF by --as-needed.
RUN dnf install -y gcc gcc-c++ make ca-certificates git pkgconf-pkg-config \
    dbus-devel libxcb-devel tar gzip diffutils findutils \
    && dnf clean all \
    && mkdir -p /work /cargo && chmod 777 /work /cargo
