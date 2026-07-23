# porter-sync.ps1 — session-start wrapper (Windows / PowerShell).
#
# VENDORED, IDENTICAL in every porter plugin. Windows counterpart of
# porter-sync.sh. Direction is passed as args by the plugin hook, e.g.
#   pwsh -File porter-sync.ps1 --source codex --target claude
# Never blocks the session (always exits 0).
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptDir 'porter-build.ps1')

$pluginRoot = Get-PorterFirstEnv @('CLAUDE_PLUGIN_ROOT', 'PLUGIN_ROOT') (Split-Path -Parent $scriptDir)
$bin = Get-PorterBin $pluginRoot
if (-not $bin) {
    # Write-Warning (not Write-Error) so $ErrorActionPreference='Stop' doesn't
    # throw before `exit 0` — the session must never be blocked.
    Write-Warning 'agent-porter: skipping porting this session.'
    exit 0
}

& $bin sync @args
exit 0
