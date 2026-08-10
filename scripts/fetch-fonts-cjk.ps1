# TUY CHON: tai bo font Noto Sans CJK (Hoa gian the/phon the + Nhat + Han)
# vao fonts/ - phuc vu tai lieu CJK khong nhung font. KHONG chay trong CI
# mac dinh (moi file ~16-20MB lam zip phinh to); nguoi dung/dev can thi chay:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-fonts-cjk.ps1
# App tu nhan dien file trong fonts/ luc khoi dong - khong can cau hinh.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$fontsDir = Join-Path $root "fonts"
New-Item -ItemType Directory -Force -Path $fontsDir | Out-Null

# Ban OTC gop 4 ngon ngu (SC/TC/JP/KR) tu repo phat hanh chinh thuc.
$files = @(
    @{ name = "NotoSansCJK-Regular.ttc"; url = "https://github.com/notofonts/noto-cjk/raw/main/Sans/OTC/NotoSansCJK-Regular.ttc" },
    @{ name = "NotoSansCJK-Bold.ttc";    url = "https://github.com/notofonts/noto-cjk/raw/main/Sans/OTC/NotoSansCJK-Bold.ttc" }
)

foreach ($f in $files) {
    $dest = Join-Path $fontsDir $f.name
    if ((Test-Path $dest) -and ((Get-Item $dest).Length -gt 1000000)) {
        Write-Output "Da co: $($f.name)"
        continue
    }
    Write-Output "Tai $($f.name) (~20MB)..."
    Invoke-WebRequest -Uri $f.url -OutFile $dest -UseBasicParsing
    if ((Get-Item $dest).Length -lt 1000000) { throw "File $($f.name) qua nho - tai hong?" }
}
Write-Output "Xong. De dong goi kem ban portable: copy fonts/ (gom ca file CJK) canh FoFreeXit.exe."
