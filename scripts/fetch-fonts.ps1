# Tai bo font Noto (hinted static TTF, phu tieng Viet) vao fonts/ o goc
# workspace - app DONG GOI bo font nay de moi may khach hien thi/ghi file
# giong het nhau, khong phu thuoc font cai tren he dieu hanh (chuan Foxit:
# khong thoa hiep thay bang font "gan giong").
#
# Dung: powershell -ExecutionPolicy Bypass -File scripts/fetch-fonts.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$fontsDir = Join-Path $root "fonts"
New-Item -ItemType Directory -Force -Path $fontsDir | Out-Null

# notofonts.github.io la repo phat hanh chinh thuc cac ban static hinted.
$base = "https://github.com/notofonts/notofonts.github.io/raw/main/fonts"
$files = @(
    "NotoSans/hinted/ttf/NotoSans-Regular.ttf",
    "NotoSans/hinted/ttf/NotoSans-Bold.ttf",
    "NotoSans/hinted/ttf/NotoSans-Italic.ttf",
    "NotoSans/hinted/ttf/NotoSans-BoldItalic.ttf",
    "NotoSansMono/hinted/ttf/NotoSansMono-Regular.ttf",
    "NotoSansMono/hinted/ttf/NotoSansMono-Bold.ttf",
    "NotoSerif/hinted/ttf/NotoSerif-Regular.ttf",
    "NotoSerif/hinted/ttf/NotoSerif-Bold.ttf",
    "NotoSerif/hinted/ttf/NotoSerif-Italic.ttf",
    "NotoSerif/hinted/ttf/NotoSerif-BoldItalic.ttf"
)

foreach ($f in $files) {
    $name = Split-Path -Leaf $f
    $dest = Join-Path $fontsDir $name
    if ((Test-Path $dest) -and ((Get-Item $dest).Length -gt 100000)) {
        Write-Output "Da co: $name"
        continue
    }
    Write-Output "Tai $name..."
    Invoke-WebRequest -Uri "$base/$f" -OutFile $dest -UseBasicParsing
    if ((Get-Item $dest).Length -lt 100000) { throw "File $name qua nho - tai hong?" }
}

# Giay phep OFL di kem khi phan phoi font.
$ofl = Join-Path $fontsDir "OFL-LICENSE.txt"
if (-not (Test-Path $ofl)) {
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/google/fonts/main/ofl/notosans/OFL.txt" -OutFile $ofl -UseBasicParsing
}

Get-ChildItem $fontsDir | ForEach-Object { "{0}  {1:N0} bytes" -f $_.Name, $_.Length } | Write-Output
Write-Output "Xong. Fonts tai: $fontsDir"
