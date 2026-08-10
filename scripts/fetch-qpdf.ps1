# Tải QPDF prebuilt (qpdf/qpdf release MSVC x64) cho Windows
# và đặt qpdf.exe + DLL phụ thuộc vào qpdf/bin/ ở gốc workspace.
#
# Dùng: powershell -ExecutionPolicy Bypass -File scripts/fetch-qpdf.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$qpdfDir = Join-Path $root "qpdf"
$tmp = Join-Path $env:TEMP "qpdf-win-msvc64.zip"

Write-Output "Tra cuu release moi nhat cua qpdf/qpdf..."
# Dung GITHUB_TOKEN khi co (CI) - goi API khong xac thuc tu runner hay dinh
# rate limit lam fail build oan. API loi thi roi ve ban pin cung.
$headers = @{}
if ($env:GITHUB_TOKEN) { $headers["Authorization"] = "Bearer $($env:GITHUB_TOKEN)" }
$dlUrl = $null
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/qpdf/qpdf/releases/latest" -Headers $headers -UseBasicParsing
    $asset = $release.assets | Where-Object { $_.name -like "*msvc64.zip" } | Select-Object -First 1
    if ($asset) { $dlUrl = $asset.browser_download_url; Write-Output "Tai $($asset.name) (release $($release.tag_name))..." }
} catch {
    Write-Output "API loi ($($_.Exception.Message)) - dung ban pin."
}
if (-not $dlUrl) {
    $dlUrl = "https://github.com/qpdf/qpdf/releases/download/v12.2.0/qpdf-12.2.0-msvc64.zip"
    Write-Output "Tai ban pin: $dlUrl"
}
Invoke-WebRequest -Uri $dlUrl -OutFile $tmp -UseBasicParsing

$extractDir = Join-Path $env:TEMP "qpdf-extract"
if (Test-Path $extractDir) { Remove-Item -Recurse -Force $extractDir }
Expand-Archive -Path $tmp -DestinationPath $extractDir -Force

$binSrc = Get-ChildItem -Path $extractDir -Recurse -Directory -Filter "bin" | Select-Object -First 1
if (-not $binSrc) { throw "Khong tim thay thu muc bin/ sau khi giai nen" }

New-Item -ItemType Directory -Force -Path (Join-Path $qpdfDir "bin") | Out-Null
Copy-Item (Join-Path $binSrc.FullName "*.exe") (Join-Path $qpdfDir "bin") -Force
Copy-Item (Join-Path $binSrc.FullName "*.dll") (Join-Path $qpdfDir "bin") -Force

$exe = Join-Path $qpdfDir "bin\qpdf.exe"
if (-not (Test-Path $exe)) { throw "Khong tim thay qpdf.exe sau khi copy ($exe)" }
& $exe --version
Write-Output "Xong. qpdf.exe da dat tai: $exe"
