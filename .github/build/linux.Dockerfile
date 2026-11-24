# The Linux release build (package.yml, ci.yml's glibc job): built against AlmaLinux 8's glibc 2.28, the programs run
# on RHEL 8, Debian 10, Ubuntu 18.10 and later (decision 0011). Dependabot moves the digest; the tag names the series.
FROM almalinux:8@sha256:2d0f2eb2a83cc09dbc2eb92fade4dbe8f709628149e81aa00aec70630406603b

# gcc for the build scripts (ring), file for appimagetool; the X11 client libraries winit loads at run time, which the
# AppImage carries ([package.metadata.packager.appimage.files] of frontends/waveduderust_slint/Cargo.toml). No -devel package:
# the windowing crates load their libraries at run time. curl, tar, xz and binutils (objdump, the glibc floor check)
# come with the base image. The packages are the repositories' of the build date; the digest pins the base only.
RUN dnf install -y --nodocs --setopt=install_weak_deps=False \
        gcc file \
        libxkbcommon libxkbcommon-x11 libxcb libXcursor libXi libXrandr \
    && dnf clean all

# rustup without a toolchain: the job installs the one rust-toolchain.toml pins. rustup-init of that release, checked
# against its published digest (static.rust-lang.org/rustup/archive/<version>/<target>/rustup-init.sha256). The homes
# are writable by any user: the job runs as the runner's, so the files it writes to the workspace stay the runner's.
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSfL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none \
    && rm /tmp/rustup-init \
    && mkdir -m 1777 /home/build \
    && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"
# Where cargo-packager caches linuxdeploy, for a user without a home.
ENV HOME=/home/build
