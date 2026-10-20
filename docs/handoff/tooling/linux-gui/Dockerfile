# Linux GUI acceptance image: installs the built .deb and drives the app through tauri-driver + WebKitWebDriver.
FROM rust:1.99.0-trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      webkit2gtk-driver xvfb xauth dbus-x11 at-spi2-core \
      fonts-noto-core fonts-noto-ui-core fonts-thai-tlwg \
      git ca-certificates curl python3 imagemagick x11-utils procps \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install tauri-driver --locked --version '^2' && cp /usr/local/cargo/bin/tauri-driver /usr/local/bin/
WORKDIR /work
