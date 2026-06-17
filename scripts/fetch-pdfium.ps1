# Tải PDFium prebuilt (bblanchon/pdfium-binaries) cho Windows x64
# và đặt pdfium.dll vào thư mục gốc workspace để ff-engine nạp được.
#
# Dùng: powershell -ExecutionPolicy Bypass -File scripts/fetch-pdfium.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$pdfiumDir = Join-Path $root "pdfium"
$tmp = Join-Path $env:TEMP "pdfium-win-x64.tgz"
$url = "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz"

Write-Output "Tải PDFium từ: $url"
Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing

New-Item -ItemType Directory -Force -Path $pdfiumDir | Out-Null
Write-Output "Giải nén vào: $pdfiumDir"
tar -xzf $tmp -C $pdfiumDir

$dll = Join-Path $pdfiumDir "bin\pdfium.dll"
if (-not (Test-Path $dll)) { throw "Không tìm thấy pdfium.dll sau khi giải nén ($dll)" }

# Copy pdfium.dll ra gốc workspace để bind_pdfium tìm thấy trong cwd.
Copy-Item $dll (Join-Path $root "pdfium.dll") -Force
Write-Output "Xong. pdfium.dll đã đặt tại gốc workspace: $(Join-Path $root 'pdfium.dll')"
