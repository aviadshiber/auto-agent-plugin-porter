# porter-bootstrap.ps1 — one-time setup for claude-to-codex (Windows).
#
# VENDORED, IDENTICAL in every porter plugin. Windows counterpart of
# porter-bootstrap.sh: build the binary, register the user-level Codex
# SessionStart hook (merge-safe; you approve trust once), and do an initial
# sync. Self-locating via $MyInvocation.
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptDir 'porter-build.ps1')

$pluginRoot = Split-Path -Parent $scriptDir
$bin = Get-PorterBin $pluginRoot
if (-not $bin) {
    # Write-Warning (not Write-Error): under $ErrorActionPreference='Stop',
    # Write-Error would throw and skip our controlled `exit 1`.
    Write-Warning 'agent-porter: cannot bootstrap without a built binary (install Rust from https://rustup.rs).'
    exit 1
}

Write-Host 'agent-porter: registering the Codex session-start hook…'
& $bin install-codex-hook --porter-bin $bin

Write-Host 'agent-porter: running an initial Claude -> Codex sync…'
& $bin sync --source claude --target codex

Write-Host ''
Write-Host 'Bootstrap complete. Codex will prompt you to TRUST the new hook on your'
Write-Host 'next session (the porter never bypasses hook trust). Re-run this bootstrap'
Write-Host 'after upgrading the plugin to rebuild the binary and refresh the hook.'
