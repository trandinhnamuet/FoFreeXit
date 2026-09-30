# Môi trường test engine + cargo check app trên Linux (dùng khi máy dev
# Windows bị Smart App Control chặn build script của cargo).
# Build:  docker build -t fofreexit-test scripts/docker-test
# Chạy:   powershell -File scripts/docker-test/run.ps1 [lệnh cargo...]
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      qpdf tesseract-ocr tesseract-ocr-eng tesseract-ocr-vie \
      fonts-dejavu fonts-liberation curl ca-certificates \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
      libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt/pdfium && \
    curl -fsSL https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz \
      | tar -xz -C /opt/pdfium
ENV FOFREEXIT_PDFIUM_PATH=/opt/pdfium/lib \
    FOFREEXIT_TESSERACT_PATH=/usr/bin/tesseract \
    FOFREEXIT_QPDF_PATH=/usr/bin/qpdf \
    CARGO_TARGET_DIR=/target
WORKDIR /src
# Kiểm tra biên dịch code riêng Windows (#[cfg(windows)]) từ Linux:
#   run.ps1 cargo check --target x86_64-pc-windows-gnu --manifest-path app/src-tauri/Cargo.toml
RUN apt-get update && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/* && rustup target add x86_64-pc-windows-gnu
