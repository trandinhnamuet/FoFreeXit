# Chạy lệnh cargo trong container Linux (máy dev Windows bị Smart App Control
# chặn build script của cargo). Mặc định: test engine.
#   powershell -File scripts/docker-test/run.ps1
#   powershell -File scripts/docker-test/run.ps1 cargo check --manifest-path app/src-tauri/Cargo.toml
# Nhiều worktree chạy song song: đặt $env:FF_TARGET_VOL khác nhau để không tranh lock.
param([Parameter(ValueFromRemainingArguments = $true)] [string[]] $Cmd)
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$vol = if ($env:FF_TARGET_VOL) { $env:FF_TARGET_VOL } else { "fofreexit-target" }
if (-not $Cmd -or $Cmd.Count -eq 0) { $Cmd = @("cargo", "test", "-p", "ff-engine", "--", "--test-threads=1") }
docker run --rm -v "${root}:/src" -v "${vol}:/target" -v fofreexit-cargo:/usr/local/cargo/registry fofreexit-test @Cmd
exit $LASTEXITCODE
