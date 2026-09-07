# Assertions for sharing/claude-mirror-mcp.mjs. Run: pwsh -NoProfile -File Test-Mirror.ps1
#
# The suite itself is Node (Test-Mirror.mjs) - JSON merge/race/JSON-parse logic is native to that
# runtime and dependency-injecting fs there is far more direct than round-tripping through
# PowerShell. This wrapper only supplies the exit-code contract every Test-*.ps1 suite shares:
# 0 pass, 1 a real failure, 2 could not run (missing node, or the script threw before it could
# even report a count).
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) { Write-Host "COULD NOT RUN: node is not on PATH"; exit 2 }

$script = Join-Path $PSScriptRoot 'Test-Mirror.mjs'
if (-not (Test-Path -LiteralPath $script)) { Write-Host "COULD NOT RUN: $script is missing"; exit 2 }

& node $script
exit $LASTEXITCODE
