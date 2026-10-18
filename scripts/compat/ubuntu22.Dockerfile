FROM ubuntu:22.04
# x11rb's allow-unsafe-code feature passes -lxcb to the linker; unused XCB
# functions are discarded from the final standalone ELF by --as-needed.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential ca-certificates git pkg-config libdbus-1-dev libxcb1-dev \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /work /cargo && chmod 777 /work /cargo
