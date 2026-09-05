# Assertions for install.ps1. Run: pwsh -File Test-Install.ps1
$ErrorActionPreference = 'Stop'
try { . "$PSScriptRoot\..\install.ps1" -Define } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }
$script:Failed = 0; $script:Ran = 0
function Assert { param([string]$Because, [bool]$Ok) $script:Ran++; if ($Ok) { Write-Host "ok    $Because" } else { Write-Host "FAIL  $Because"; $script:Failed++ } }
$tmp = Join-Path $env:TEMP ("cal-install-" + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory $tmp | Out-Null
try {
    $cfg = Join-Path $tmp 'claude-auto.json'
    Assert 'writes a default config when absent'  ((Write-DefaultLauncherConfig -Path $cfg) -eq 'created' -and (Test-Path $cfg))
    Assert 'never overwrites an existing config'  ((Write-DefaultLauncherConfig -Path $cfg) -eq 'kept')
    Assert 'the default config parses to one account' (@((Get-Content $cfg -Raw | ConvertFrom-Json).accounts).Count -eq 1)
    Assert 'PATH add is idempotent'                ((Add-PathEntry -Current 'C:\a;C:\b' -Entry 'C:\b') -eq 'C:\a;C:\b')
    Assert 'PATH add appends'                      ((Add-PathEntry -Current 'C:\a' -Entry 'C:\b') -eq 'C:\a;C:\b')
    Assert 'PATH add keeps an unexpanded %VAR% entry intact' ((Add-PathEntry -Current '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps;X:\a' -Entry 'X:\b') -eq '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps;X:\a;X:\b')
    Assert 'requirements report names pwsh and claude' ((Get-InstallRequirements).Keys -contains 'pwsh' -and (Get-InstallRequirements).Keys -contains 'claude')
} finally { Remove-Item $tmp -Recurse -Force }
if ($script:Ran -ne 7) { Write-Host "COULD NOT RUN: expected 7 assertions, ran $($script:Ran)"; exit 2 }
if ($script:Failed) { Write-Host "$($script:Failed) failed"; exit 1 }
Write-Host "Test-Install green ($($script:Ran) assertions)"; exit 0
