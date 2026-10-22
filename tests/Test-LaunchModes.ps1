# Assertions for LaunchModes.ps1 (the launch-mode table). Run: pwsh -File Test-LaunchModes.ps1
#
# The table is read by the project screen, its hotkeys, Get-LaunchArgs and the launcher loop; what
# those readers do with it is asserted in Test-Ui and the preview check. This suite pins the table's
# own guards: a hotkey clash refuses to load, and every hotkey has its Cyrillic-layout twin.
try {
    . "$PSScriptRoot\..\claude-auto\LaunchModes.ps1"
    . "$PSScriptRoot\..\claude-auto\Input.ps1"
} catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

$script:Failed = 0
$script:Ran = 0
function Assert-Equal {
    param($Expected, $Actual, [string]$Because)
    $script:Ran++
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL  $Because"
        Write-Host "      expected: $Expected"
        Write-Host "      actual:   $Actual"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}
function New-Mode { param([string]$Name, [string]$Hotkey) [pscustomobject]@{ Name = $Name; Hotkey = $Hotkey } }

# --- the shipped table ------------------------------------------------------------------------------
Assert-Equal '' ((Get-LaunchModeTableErrors -Modes (Get-LaunchModes)) -join '; ') 'given the shipped table, when checked, then it has no hotkey clash'
Assert-Equal 'new,continue,resume,worktree' (@(Get-LaunchModes | ForEach-Object { $_.Name }) -join ',') 'the rows are the four modes, in the field order'
Assert-Equal 'resume' (Get-LaunchMode -Name 'ReSuMe').Name 'a lookup is case-insensitive, like the switch it replaced'
Assert-Equal $true ($null -eq (Get-LaunchMode -Name 'bogus')) 'an unknown name has no row'

# --- clashes ----------------------------------------------------------------------------------------
$slash = @(Get-LaunchModeTableErrors -Modes @((New-Mode 'new' ''), (New-Mode 'filterish' '/')))
Assert-Equal 1 $slash.Count 'given a mode on /, when checked, then one error'
Assert-Equal $true ($slash[0] -match "mode 'filterish'" -and $slash[0] -match "'/'") 'and it names the row and the key'
foreach ($nav in @('w', 'a', 's', 'd')) {
    $navErr = @(Get-LaunchModeTableErrors -Modes @(New-Mode 'nav' $nav))
    Assert-Equal 1 $navErr.Count "a mode on '$nav' is refused - it would disable that navigation alias (Ui.ps1 loopAlias)"
}
$dup = @(Get-LaunchModeTableErrors -Modes @((New-Mode 'one' 'x'), (New-Mode 'two' 'x')))
Assert-Equal 1 $dup.Count 'given two modes on one key, when checked, then one error'
Assert-Equal $true ($dup[0] -match "mode 'two'" -and $dup[0] -match "mode 'one'") 'and it names both rows'
Assert-Equal 0 @(Get-LaunchModeTableErrors -Modes @((New-Mode 'a' ''), (New-Mode 'b' ''))).Count 'rows without a hotkey never clash'

# --- a clashing table refuses to LOAD ----------------------------------------------------------------
# In a child process: the file assigns the table before it checks it, so loading a bad copy here
# would replace this suite's own table.
$badDir = Join-Path ([IO.Path]::GetTempPath()) ('launchmodes-test-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$null = New-Item -ItemType Directory -Path $badDir
try {
    $badFile = Join-Path $badDir 'LaunchModes.ps1'
    $src = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '..\claude-auto\LaunchModes.ps1'))
    [IO.File]::WriteAllText($badFile, $src.Replace("Name = 'worktree'; Hotkey = 't'", "Name = 'worktree'; Hotkey = '/'"))
    $loadOut = (& pwsh -NoProfile -Command "try { . '$badFile'; 'loaded' } catch { `$_.Exception.Message }" 2>&1) -join ' '
    Assert-Equal $true ($loadOut -match "LaunchModes\.ps1: mode 'worktree': hotkey '/'") "given a table with worktree on '/', when loaded, then it throws naming the row and the key (got: $loadOut)"
} finally {
    Remove-Item -LiteralPath $badDir -Recurse -Force -ErrorAction SilentlyContinue
}

# --- every hotkey works on the Cyrillic layouts -------------------------------------------------------
# Test-ClaudeHotkey matches a VK_PACKET press (RDP soft keyboard, paste) only through
# Input.ps1 $script:HotkeyLayoutChars; a mode hotkey missing there is dead on a Cyrillic layout.
foreach ($mode in @(Get-LaunchModes | Where-Object { $_.Hotkey })) {
    Assert-Equal $true ($script:HotkeyLayoutChars.ContainsKey($mode.Hotkey)) "mode '$($mode.Name)' hotkey '$($mode.Hotkey)' has its Cyrillic-layout letter in Input.ps1 HotkeyLayoutChars"
}

$expectedRan = 14 + @(Get-LaunchModes | Where-Object { $_.Hotkey }).Count
if ($script:Ran -ne $expectedRan) { Write-Host "COULD NOT RUN: expected $expectedRan assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
exit 0
