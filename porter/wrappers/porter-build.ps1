# porter-build.ps1 — shared "ensure the porter binary is built" helper (Windows).
#
# VENDORED, IDENTICAL in every porter plugin. Dot-sourced by porter-sync.ps1 and
# porter-bootstrap.ps1. Returns the path to a ready-to-run binary, building it
# once (first run / source change) under the plugin data dir.

function Get-PorterFirstEnv([string[]]$names, [string]$fallback) {
    foreach ($n in $names) {
        $v = [Environment]::GetEnvironmentVariable($n)
        if ($v) { return $v }
    }
    return $fallback
}

# Ensures the binary exists and is current; returns its path, or $null on
# failure (caller decides whether to exit 0).
function Get-PorterBin([string]$pluginRoot) {
    $crateDir = Join-Path $pluginRoot 'porter'
    $cacheBase = Get-PorterFirstEnv @('CLAUDE_PLUGIN_DATA', 'PLUGIN_DATA', 'LOCALAPPDATA') (Join-Path $env:USERPROFILE '.cache')
    $dataDir = Join-Path $cacheBase 'auto-agent-plugin-porter'
    $binDir = Join-Path $dataDir 'bin'
    $bin = Join-Path $binDir 'agent-porter.exe'
    $stamp = Join-Path $binDir '.src-sha'
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    function Script:Get-SrcHash {
        $files = @()
        foreach ($f in @('Cargo.toml', 'Cargo.lock')) {
            $p = Join-Path $crateDir $f
            if (Test-Path $p) { $files += $p }
        }
        $srcDir = Join-Path $crateDir 'src'
        if (Test-Path $srcDir) {
            $files += (Get-ChildItem -Recurse -File $srcDir | Sort-Object FullName | ForEach-Object { $_.FullName })
        }
        $sha = [System.Security.Cryptography.SHA256]::Create()
        $acc = [System.IO.MemoryStream]::new()
        foreach ($p in $files) {
            $bytes = [System.IO.File]::ReadAllBytes($p)
            $acc.Write($bytes, 0, $bytes.Length)
        }
        $acc.Position = 0
        ($sha.ComputeHash($acc) | ForEach-Object { $_.ToString('x2') }) -join ''
    }

    $needBuild = $false
    if (-not (Test-Path $bin)) {
        $needBuild = $true
    } elseif (-not (Test-Path $stamp) -or ((Get-Content -Raw $stamp).Trim() -ne (Get-SrcHash))) {
        $needBuild = $true
    }

    if ($needBuild) {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
            Write-Error 'agent-porter: Rust toolchain not found — install from https://rustup.rs.'
            return $null
        }
        Write-Host 'agent-porter: building the porter binary (first run or source changed)…'
        Push-Location $crateDir
        try {
            cargo build --release --quiet
            if ($LASTEXITCODE -ne 0) { Write-Error 'agent-porter: build failed.'; return $null }
        } finally {
            Pop-Location
        }
        Copy-Item (Join-Path $crateDir 'target/release/agent-porter.exe') $bin -Force
        Get-SrcHash | Set-Content -NoNewline $stamp
    }
    return $bin
}
