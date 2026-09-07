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

    # deferred review finding: nothing pinned CLAUDE_AUTO_CONFIG's own '~' expansion (only an
    # already-absolute override, above). Get-LauncherConfigPath must run it through
    # Expand-LauncherPath like every other path in this file.
    $env:CLAUDE_AUTO_CONFIG = '~/tilde-config-test.json'
    Assert 'env override expands a leading ~'     ((Get-LauncherConfigPath) -eq (Join-Path $HOME 'tilde-config-test.json'))

    Remove-Item Env:CLAUDE_AUTO_CONFIG
    Assert 'default path'                         ((Get-LauncherConfigPath) -eq (Join-Path $HOME '.claude\claude-auto.json'))

    # --- deferred review finding: relative paths must resolve against $PWD, not the process's
    # unmanaged CWD. Set-Location never touches [Environment]::CurrentDirectory, so the two can
    # point at different directories; GetFullPath(path) alone reads the latter. ---
    $otherCwd = Join-Path $tmp 'other-cwd'; New-Item -ItemType Directory $otherCwd | Out-Null
    $trueCwd = Join-Path $tmp 'true-cwd'; New-Item -ItemType Directory $trueCwd | Out-Null
    $savedNetCwd = [Environment]::CurrentDirectory
    [Environment]::CurrentDirectory = $otherCwd
    Push-Location $trueCwd
    try {
        $expanded = Expand-LauncherPath 'rel\thing.json'
        Assert 'relative path expands against $PWD, not [Environment]::CurrentDirectory' ($expanded -eq (Join-Path $trueCwd 'rel\thing.json'))
    } finally { Pop-Location; [Environment]::CurrentDirectory = $savedNetCwd }

    # --- deferred review finding: string-typed JSON booleans. [bool]"false" is $true in PowerShell,
    # so "sharing": "false" used to turn sharing ON. ---
    Set-Content (Join-Path $tmp 'bool-sharing-false.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "sharing": "false" }'
    $bsf = Read-LauncherConfig -Path (Join-Path $tmp 'bool-sharing-false.json')
    Assert 'string "false" sharing stays off'          (-not $bsf.Sharing)
    Assert 'string "false" sharing warns nothing'      (@($bsf.Warnings).Count -eq 0)

    Set-Content (Join-Path $tmp 'bool-sharing-bad.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "sharing": "yes" }'
    $bsb = Read-LauncherConfig -Path (Join-Path $tmp 'bool-sharing-bad.json')
    Assert 'unparseable sharing value keeps the default' (-not $bsb.Sharing)
    Assert 'and warns naming the key and the value'      (@($bsb.Warnings | Where-Object { $_ -match 'sharing' -and $_ -match 'yes' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'bool-hidden.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"b","root":"~/.claude-b","hidden":"false"} ] }'
    $bhf = Read-LauncherConfig -Path (Join-Path $tmp 'bool-hidden.json')
    Assert 'string "false" hidden stays not-hidden'    (($bhf.Accounts | Where-Object Key -eq 'b').Hidden -eq $false)

    Set-Content (Join-Path $tmp 'bool-hidden-true.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"b","root":"~/.claude-b","hidden":"true"} ] }'
    $bht = Read-LauncherConfig -Path (Join-Path $tmp 'bool-hidden-true.json')
    Assert 'string "true" hidden is hidden'             (($bht.Accounts | Where-Object Key -eq 'b').Hidden -eq $true)

    # --- deferred review finding: "accounts": null must warn, like every other invalid accounts
    # value does - it currently falls back to the default roster silently. ---
    Set-Content (Join-Path $tmp 'null-accounts.json') '{ "accounts": null }'
    $na = Read-LauncherConfig -Path (Join-Path $tmp 'null-accounts.json')
    Assert 'accounts:null uses the default roster' (@($na.Accounts).Count -eq 1 -and $na.Accounts[0].Key -eq 'work')
    Assert 'and warns, unlike every other invalid accounts value' (@($na.Warnings | Where-Object { $_ -match 'accounts' -and $_ -match 'null' }).Count -eq 1)

    # --- deferred review finding: two accounts sharing a tint must not fail the roster, but the
    # collision must be named in a warning - colour is the only way the owner tells accounts apart. ---
    Set-Content (Join-Path $tmp 'duptint.json') '{ "accounts": [ {"key":"work","root":"~/.claude","tint":"Green"}, {"key":"b","root":"~/.claude-b","tint":"Green"} ] }'
    $dt = Read-LauncherConfig -Path (Join-Path $tmp 'duptint.json')
    Assert 'duplicate tints keep both accounts'   (@($dt.Accounts).Count -eq 2)
    Assert 'and warn naming the colliding keys'   (@($dt.Warnings | Where-Object { $_ -match 'work' -and $_ -match 'b' -and $_ -match 'tint' }).Count -eq 1)

    # --- deferred review finding: tint case is normalised to the allowed list's canonical casing,
    # so a lower-case value stored verbatim cannot end up looked up under the wrong key elsewhere. ---
    Set-Content (Join-Path $tmp 'tintcase.json') '{ "accounts": [ {"key":"work","root":"~/.claude","tint":"magenta"} ] }'
    $tc = Read-LauncherConfig -Path (Join-Path $tmp 'tintcase.json')
    Assert 'lower-case tint is normalised to canonical casing' ($tc.Accounts[0].Tint -ceq 'Magenta')
    Assert 'a valid colour in any case warns nothing'          (@($tc.Warnings | Where-Object { $_ -match 'tint' }).Count -eq 0)

    # --- deferred review finding: a second maintenance action reusing an existing key must be
    # skipped with a warning naming the key, not silently accepted alongside the first. ---
    Set-Content (Join-Path $tmp 'dupmaint.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "maintenanceActions": [ {"key":"i","label":"first","script":"C:\\x.ps1"}, {"key":"i","label":"second","script":"C:\\y.ps1"} ] }'
    $dm = Read-LauncherConfig -Path (Join-Path $tmp 'dupmaint.json')
    Assert 'duplicate maintenance key: the first one wins'   (@($dm.MaintenanceActions).Count -eq 1 -and $dm.MaintenanceActions[0].Label -eq 'first')
    Assert 'and a warning names the key'                     (@($dm.Warnings | Where-Object { $_ -match 'maintenanceActions' -and $_ -match "'i'" }).Count -eq 1)
    # --- deferred review finding: launchHooks, extraMcpConfigs and maintenanceActions[].script must
    # each be absolute, like accounts[].root and secretsRoot already are - a relative value resolves
    # against $PWD, so the same config would run a different hook depending on launch directory. ---
    Set-Content (Join-Path $tmp 'hook-rel.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "launchHooks": ["rel/hook.ps1", "~/ok/hook.ps1"] }'
    $hr = Read-LauncherConfig -Path (Join-Path $tmp 'hook-rel.json')
    Assert 'relative launchHooks entry is skipped'        (@($hr.LaunchHooks).Count -eq 1 -and $hr.LaunchHooks[0] -eq (Join-Path $HOME 'ok\hook.ps1'))
    Assert 'and a warning names it as not absolute'       (@($hr.Warnings | Where-Object { $_ -match 'launchHooks' -and $_ -match 'absolute' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'extra-rel.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "extraMcpConfigs": ["rel/x.json", "C:\\ok\\x.json"] }'
    $er = Read-LauncherConfig -Path (Join-Path $tmp 'extra-rel.json')
    Assert 'relative extraMcpConfigs entry is skipped'    (@($er.ExtraMcpConfigs).Count -eq 1 -and $er.ExtraMcpConfigs[0] -eq 'C:\ok\x.json')
    Assert 'and a warning names it as not absolute'       (@($er.Warnings | Where-Object { $_ -match 'extraMcpConfigs' -and $_ -match 'absolute' }).Count -eq 1)

    Set-Content (Join-Path $tmp 'maint-rel.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "maintenanceActions": [ {"key":"i","script":"rel/x.ps1"} ] }'
    $mr = Read-LauncherConfig -Path (Join-Path $tmp 'maint-rel.json')
    Assert 'maintenanceActions with a relative script is skipped'   (@($mr.MaintenanceActions).Count -eq 0)
    Assert 'and a warning names the key and absolute path'          (@($mr.Warnings | Where-Object { $_ -match 'maintenanceActions' -and $_ -match "'i'" -and $_ -match 'absolute' }).Count -eq 1)

    # --- friend-trial finding 1: a typo'd top-level key ("account" instead of "accounts") vanished
    # with no warning at all - Read-LauncherConfig only looks at keys it knows. Unrecognized
    # top-level keys must warn, once each, naming the key and suggesting a near-miss real key. ---
    Set-Content (Join-Path $tmp 'typo-key.json') '{ "account": [ {"key":"work","root":"~/.claude"} ] }'
    $tk = Read-LauncherConfig -Path (Join-Path $tmp 'typo-key.json')
    Assert 'typo top-level key warns once, naming it'            (@($tk.Warnings | Where-Object { $_ -match "'account'" }).Count -eq 1)
    Assert 'and suggests the near-miss real key'                 (@($tk.Warnings | Where-Object { $_ -match "'account'" -and $_ -match "'accounts'" }).Count -eq 1)

    Set-Content (Join-Path $tmp 'valid-keys.json') '{ "accounts": [ {"key":"work","root":"~/.claude"} ], "sharing": false }'
    $vk = Read-LauncherConfig -Path (Join-Path $tmp 'valid-keys.json')
    Assert 'a config with only valid keys warns nothing about unknown keys' (@($vk.Warnings | Where-Object { $_ -match 'unrecognized' }).Count -eq 0)

    # --- friend-trial finding 2: a structural roster failure discarded the whole roster and named
    # no account. A relative root, or a first-letter collision, must name the offending account(s)
    # and the offending value. ---
    Set-Content (Join-Path $tmp 'rel2.json') '{ "accounts": [ {"key":"work","root":"~/.claude"}, {"key":"b","root":".claude-relative"} ] }'
    $rel2 = Read-LauncherConfig -Path (Join-Path $tmp 'rel2.json')
    Assert 'relative-root warning names the offending account key'   (@($rel2.Warnings | Where-Object { $_ -match "'b'" -and $_ -match 'absolute' }).Count -eq 1)
    Assert 'and the offending raw value'                            (@($rel2.Warnings | Where-Object { $_ -match [regex]::Escape('.claude-relative') }).Count -eq 1)

    Assert 'first-letter collision warning names both colliding keys' (@($bad.Warnings | Where-Object { $_ -match "work" -and $_ -match "web" -and $_ -match 'first letter' }).Count -eq 1)

    # --- friend-trial finding 4: a Windows path typed straight into JSON ("C:\Users\...") fails
    # ConvertFrom-Json with a raw .NET exception and no hint that a single backslash is the cause. ---
    Set-Content (Join-Path $tmp 'winpath.json') '{ "accounts": [ {"key":"work","root":"C:\Users\me\.claude"} ] }'
    $wp = Read-LauncherConfig -Path (Join-Path $tmp 'winpath.json')
    Assert 'a single backslash JSON parse failure names it as the likely cause' (@($wp.Warnings | Where-Object { $_ -match 'backslash' }).Count -eq 1)
} finally { Remove-Item $tmp -Recurse -Force }
if ($script:Ran -eq 0) { Write-Host 'COULD NOT RUN: no assertion executed'; exit 2 }
if ($script:Failed) { Write-Host "$($script:Failed) failed"; exit 1 }
Write-Host "Test-Config green ($($script:Ran) assertions)"; exit 0
