FROM rustembedded/cross:aarch64-unknown-linux-gnu-0.2.1

RUN dpkg --add-architecture arm64 && \
    apt-get update && apt-get install -y \
    libgl1-mesa-dev:arm64 \
    libx11-xcb-dev:arm64
