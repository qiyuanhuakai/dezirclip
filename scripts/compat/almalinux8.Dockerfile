FROM almalinux:8
COPY almalinux8-repositories.sh /tmp/almalinux8-repositories.sh
# x11rb's allow-unsafe-code feature passes -lxcb to the linker; unused XCB
# functions are discarded from the final standalone ELF by --as-needed.
RUN . /tmp/almalinux8-repositories.sh \
    && almalinux_dnf install -y gcc gcc-c++ make ca-certificates git pkgconf-pkg-config \
    dbus-devel libxcb-devel tar gzip diffutils findutils \
    && dnf clean all \
    && mkdir -p /work /cargo && chmod 777 /work /cargo
