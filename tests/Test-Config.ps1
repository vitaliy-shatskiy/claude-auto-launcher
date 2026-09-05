# Assertions for Config.ps1. Run: pwsh -File Test-Config.ps1
$ErrorActionPreference = 'Stop'
try { . "$PSScriptRoot\..\claude-auto\Config.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }
$script:Failed = 0; $script:Ran = 0
function Assert { param([string]$Because, [bool]$Ok) $script:Ran++; if ($Ok) { Write-Host "ok    $Because" } else { Write-Host "FAIL  $Because"; $script:Failed++ } }
$tmp = Join-Path $env:TEMP ("cal-config-" + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory $tmp | Out-Null
try {
    $d = Read-LauncherConfig -Path (Join-Path $tmp 'missing.json')
    Assert 'no file: one account'                 (@($d.Accounts).Count -eq 1)
    Assert 'no file: the account is canonical'    ($d.Accounts[0].Canonical -and $d.Accounts[0].Key -eq 'work')
    Assert 'no file: root is ~/.claude expanded'  ($d.Accounts[0].Root -eq (Join-Path $HOME '.claude'))
    Assert 'no file: sharing and remote off'      (-not $d.Sharing -and -not $d.Remote)
    Assert 'no file: rider auto'                  ($d.RiderMcp -eq 'auto')
    Assert 'no file: no warnings'                 (@($d.Warnings).Count -eq 0)

    $four = Read-LauncherConfig -Path "$PSScriptRoot\fixtures\config-four.json"
    Assert 'four accounts read'                   (@($four.Accounts).Count -eq 4)
    Assert 'hidden flag read'                     ($four.Accounts[2].Hidden -eq $true -and $four.Accounts[0].Hidden -eq $false)
    Assert 'canonical is the ~/.claude entry'     (@($four.Accounts | Where-Object Canonical).Key -eq 'work')
    Assert 'tilde expands in extras'              ($four.ExtraMcpConfigs[0] -eq (Join-Path $HOME '.claude\mcp-shared.json'))
    Assert 'maintenance action read'              ($four.MaintenanceActions[0].Key -eq 'i' -and $four.MaintenanceActions[0].ConfirmTwice)
    Assert 'sharing and remote read from file'    ($four.Sharing -eq $true -and $four.Remote -eq $true)

    New-Item -ItemType Directory (Join-Path $tmp 'dir.json') | Out-Null
    $savedEap = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $dir = Read-LauncherConfig -Path (Join-Path $tmp 'dir.json') 2>$null } finally { $ErrorActionPreference = $savedEap }
    Assert 'unreadable (directory) → defaults + warning' (@($dir.Accounts).Count -eq 1 -and @($dir.Warnings | Where-Object { $_ -match 'could not be read' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'share1.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "sharing": true }'
    $s1 = Read-LauncherConfig -Path (Join-Path $tmp 'share1.json')
    Assert 'sharing with one account → off + warning' (-not $s1.Sharing -and @($s1.Warnings | Where-Object { $_ -match 'two accounts' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'dup.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"work","root":"~/.claude-b"} ] }'
    $dup = Read-LauncherConfig -Path (Join-Path $tmp 'dup.json')
    Assert 'duplicate keys → default roster + warning' (@($dup.Accounts).Count -eq 1 -and @($dup.Warnings | Where-Object { $_ -match 'unique' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'rel.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"b","root":"rel/path"} ] }'
    $rel = Read-LauncherConfig -Path (Join-Path $tmp 'rel.json')
    Assert 'relative root → default roster + warning' (@($rel.Accounts).Count -eq 1 -and @($rel.Warnings | Where-Object { $_ -match 'absolute' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'maint.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "maintenanceActions": [ {"key":"u","script":"x.ps1"}, {"key":"zz","script":"x.ps1"}, {"key":"q"} ] }'
    $mt = Read-LauncherConfig -Path (Join-Path $tmp 'maint.json')
    Assert 'invalid maintenance actions skipped'   (@($mt.MaintenanceActions).Count -eq 0 -and @($mt.Warnings | Where-Object { $_ -match 'maintenanceActions' }).Count -eq 3)

    Set-Content (Join-Path $tmp 'maint-key.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "maintenanceActions": [ {"key":"[","script":"x.ps1"} ] }'
    $mk = Read-LauncherConfig -Path (Join-Path $tmp 'maint-key.json')
    Assert 'action key outside [a-z0-9] skipped'      (@($mk.MaintenanceActions).Count -eq 0 -and @($mk.Warnings | Where-Object { $_ -match 'maintenanceActions' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'bad.json') '{ "accounts": [ {"key":"work","root":"~/.claude","tint":"Green"}, {"key":"web","root":"~/.claude-b","tint":"Pink"} ], "riderMcp": "maybe" }'
    $bad = Read-LauncherConfig -Path (Join-Path $tmp 'bad.json')
    Assert 'duplicate first letter → default roster' (@($bad.Accounts).Count -eq 1)
    Assert 'a warning names the collision'        (@($bad.Warnings | Where-Object { $_ -match 'first letter' }).Count -eq 1)
    Assert 'bad riderMcp falls back to auto'      ($bad.RiderMcp -eq 'auto' -and @($bad.Warnings | Where-Object { $_ -match 'riderMcp' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'tint.json') '{ "accounts": [ {"key":"work","root":"~/.claude","tint":"Pink"} ] }'
    $t = Read-LauncherConfig -Path (Join-Path $tmp 'tint.json')
    Assert 'bad tint → Green + warning'           ($t.Accounts[0].Tint -eq 'Green' -and @($t.Warnings | Where-Object { $_ -match 'tint' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'nocanon.json') '{ "accounts": [ {"key":"a","root":"~/.claude-a","tint":"Green"} ] }'
    $n = Read-LauncherConfig -Path (Join-Path $tmp 'nocanon.json')
    Assert 'no ~/.claude entry → default roster + warning' ($n.Accounts[0].Key -eq 'work' -and @($n.Warnings | Where-Object { $_ -match 'canonical' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'long.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"nine-chars","root":"~/.claude-x"} ] }'
    $l = Read-LauncherConfig -Path (Join-Path $tmp 'long.json')
    Assert 'key longer than 8 → default roster'   (@($l.Accounts).Count -eq 1 -and @($l.Warnings | Where-Object { $_ -match '8 characters' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'secrets.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "secretsRoot": "~/my-secrets" }'
    $sr = Read-LauncherConfig -Path (Join-Path $tmp 'secrets.json')
    Assert 'secretsRoot expands tilde'            ($sr.SecretsRoot -eq (Join-Path $HOME 'my-secrets'))
    Assert 'no secretsRoot: a directory beside the profile' ((Get-LauncherDefaults).SecretsRoot -eq (Join-Path $HOME '.claude-secrets'))

    Set-Content (Join-Path $tmp 'secrets-empty.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "secretsRoot": "  " }'
    $se = Read-LauncherConfig -Path (Join-Path $tmp 'secrets-empty.json')
    Assert 'empty secretsRoot keeps the default'  ($se.SecretsRoot -eq (Get-LauncherDefaults).SecretsRoot)
    Assert 'empty secretsRoot warns'              (@($se.Warnings | Where-Object { $_ -match 'secretsRoot is empty' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'secrets-rel.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "secretsRoot": "rel/dir" }'
    $sre = Read-LauncherConfig -Path (Join-Path $tmp 'secrets-rel.json')
    Assert 'relative secretsRoot keeps the default' ($sre.SecretsRoot -eq (Get-LauncherDefaults).SecretsRoot)
    Assert 'relative secretsRoot warns absolute'    (@($sre.Warnings | Where-Object { $_ -match 'secretsRoot must be an absolute path' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'garbage.json') '{ not json'
    $g = Read-LauncherConfig -Path (Join-Path $tmp 'garbage.json')
    Assert 'unparsable → defaults + warning'      (@($g.Accounts).Count -eq 1 -and @($g.Warnings).Count -eq 1)

    $env:CLAUDE_AUTO_CONFIG = Join-Path $tmp 'nope.json'
    Assert 'env override wins'                    ((Get-LauncherConfigPath) -eq $env:CLAUDE_AUTO_CONFIG)
    Remove-Item Env:CLAUDE_AUTO_CONFIG
    Assert 'default path'                         ((Get-LauncherConfigPath) -eq (Join-Path $HOME '.claude\claude-auto.json'))
} finally { Remove-Item $tmp -Recurse -Force }
if ($script:Ran -eq 0) { Write-Host 'COULD NOT RUN: no assertion executed'; exit 2 }
if ($script:Failed) { Write-Host "$($script:Failed) failed"; exit 1 }
Write-Host "Test-Config green ($($script:Ran) assertions)"; exit 0
