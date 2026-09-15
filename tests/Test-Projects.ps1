# Assertions for Projects.ps1. Run: pwsh -File Test-Projects.ps1
try {
    . "$PSScriptRoot\..\claude-auto\Sessions.ps1"
    . "$PSScriptRoot\..\claude-auto\Projects.ps1"
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
function Assert-True {
    param([bool]$Actual, [string]$Because)
    $script:Ran++
    if (-not $Actual) {
        Write-Host "FAIL  $Because"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}

$root = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$dir  = Join-Path $root 'C--Users-x-Projects-Acme-Dashboard-Web-verdis-reports'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$real = 'C:\Users\x\Projects\Acme.Dashboard.Web-verdis-reports'
$rec  = @{ type='user'; cwd=$real; sessionId='s1'; timestamp='2026-09-09T10:00:00Z' } | ConvertTo-Json -Compress
Set-Content -LiteralPath (Join-Path $dir 's1.jsonl') -Value $rec -Encoding utf8 -NoNewline

Assert-Equal $real (Get-ProjectPathFromTranscript -Directory $dir) 'cwd comes from the transcript'
# The RED that matters: the old slug heuristic cannot produce this.
Assert-Equal (ConvertFrom-ClaudeProjectSlug -Slug (Split-Path $dir -Leaf)).Project `
             'Acme-Dashboard-Web-verdis-reports' 'slug heuristic loses the dots'
Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

function New-ProjFixture {
    param([string]$Root, [string]$Slug, [string]$RealPath, [datetime]$When)
    $d = Join-Path $Root $Slug
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    if ($RealPath) {
        $f = Join-Path $d 's.jsonl'
        Set-Content -LiteralPath $f -Encoding utf8 -NoNewline -Value (
            @{ type='user'; cwd=$RealPath } | ConvertTo-Json -Compress)
        (Get-Item -LiteralPath $f).LastWriteTime = $When
    }
    return $d
}

$root2 = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj2-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$here  = (Get-Location).Path
New-ProjFixture -Root $root2 -Slug 'A' -RealPath $here          -When (Get-Date).AddMinutes(-5)  | Out-Null
New-ProjFixture -Root $root2 -Slug 'B' -RealPath $PSScriptRoot  -When (Get-Date).AddDays(-2)     | Out-Null
New-ProjFixture -Root $root2 -Slug 'C' -RealPath 'C:\no\such\dir\at\all' -When (Get-Date)        | Out-Null
New-ProjFixture -Root $root2 -Slug 'D' -RealPath $null          -When (Get-Date)                 | Out-Null

$cache = Join-Path $root2 'cache.json'
$reg = @(Get-ProjectRegistry -ProjectsRoot $root2 -CachePath $cache)

Assert-Equal 2 $reg.Count 'only resolvable, existing projects are listed'
Assert-Equal 'A' $reg[0].Slug 'newest activity first'
Assert-Equal 'B' $reg[1].Slug 'older second'
Assert-Equal $here $reg[0].Path 'path comes from the transcript'
Assert-True (Test-Path -LiteralPath $cache) 'cache file written'

# Deleting the underlying jsonl removes the directory's newest-file entirely, so the loop's own
# 'if (-not $newest) { continue }' drops the project before the cache is ever consulted - this
# exercises THAT guard, not the cache. The cache itself is proven by the two cases below.
Remove-Item -LiteralPath (Join-Path $root2 'B\s.jsonl') -Force
$reg2 = @(Get-ProjectRegistry -ProjectsRoot $root2 -CachePath $cache)
Assert-Equal 0 (@($reg2 | Where-Object { $_.Slug -eq 'B' }).Count) 'a project with no transcript left drops out'
Remove-Item -LiteralPath $root2 -Recurse -Force -ErrorAction SilentlyContinue

# (a) A real cache HIT: the key is <slug>|<mtime ticks>, so rewriting the transcript's content while
# preserving its mtime must NOT change what the registry reports - it has to answer from the cache,
# not re-read the file. Two DISTINCT real directories, never $here vs $PSScriptRoot: run from inside
# tests\ those two are the SAME path, and the assertion would hold no matter what the cache did.
$root3    = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj3-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$hitPath1 = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-hit1-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$hitPath2 = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-hit2-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $hitPath1 | Out-Null
New-Item -ItemType Directory -Force -Path $hitPath2 | Out-Null
$hitDir = New-ProjFixture -Root $root3 -Slug 'Hit' -RealPath $hitPath1 -When (Get-Date)
$cache3 = Join-Path $root3 'cache3.json'

$reg3a = @(Get-ProjectRegistry -ProjectsRoot $root3 -CachePath $cache3)
Assert-Equal $hitPath1 ($reg3a | Where-Object { $_.Slug -eq 'Hit' }).Path 'first call resolves the path from the transcript'

$hitFile = Join-Path $hitDir 's.jsonl'
$hitWhen = (Get-Item -LiteralPath $hitFile).LastWriteTime
Set-Content -LiteralPath $hitFile -Encoding utf8 -NoNewline -Value (
    @{ type='user'; cwd=$hitPath2 } | ConvertTo-Json -Compress)
(Get-Item -LiteralPath $hitFile).LastWriteTime = $hitWhen   # same key: the cache must answer, not the new content

$reg3b = @(Get-ProjectRegistry -ProjectsRoot $root3 -CachePath $cache3)
Assert-Equal $hitPath1 ($reg3b | Where-Object { $_.Slug -eq 'Hit' }).Path 'a cache hit (same mtime) returns the cached path, not a re-read of the changed transcript'
Remove-Item -LiteralPath $root3 -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $hitPath1 -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $hitPath2 -Recurse -Force -ErrorAction SilentlyContinue

# (b) A corrupt cache file must degrade to a rebuild, never a thrown error - the same rule
# Get-ClaudeSessions already follows for its own cache.
$root4 = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj4-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-ProjFixture -Root $root4 -Slug 'X' -RealPath $here -When (Get-Date) | Out-Null
$cache4 = Join-Path $root4 'corrupt.json'
Set-Content -LiteralPath $cache4 -Value '{' -Encoding utf8

$threw4 = $false
try { $reg4 = @(Get-ProjectRegistry -ProjectsRoot $root4 -CachePath $cache4) } catch { $threw4 = $true }
Assert-Equal $false $threw4 'a corrupt cache file does not throw'
Assert-Equal 1 $reg4.Count 'and the registry rebuilds from the transcripts instead'
Remove-Item -LiteralPath $root4 -Recurse -Force -ErrorAction SilentlyContinue

# CLAUDE_AUTO_PROJECTS_ROOT (fix round 1, SURVIVING MUTANT): overrides Get-ProjectRegistry's
# default -ProjectsRoot the same way CLAUDE_AUTO_PREFS overrides Get-LaunchPrefsPath's - so
# check-preview.ps1's fixture can point the project screen at an isolated tree instead of the
# owner's real ~/.claude/projects, without a caller having to pass -ProjectsRoot by hand.
$envRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-envroot-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-ProjFixture -Root $envRoot -Slug 'EnvA' -RealPath $here -When (Get-Date) | Out-Null
$savedEnvRoot = $env:CLAUDE_AUTO_PROJECTS_ROOT
try {
    $env:CLAUDE_AUTO_PROJECTS_ROOT = $envRoot
    $envCache = Join-Path $envRoot 'cache-env.json'
    $envReg = @(Get-ProjectRegistry -CachePath $envCache)
    Assert-Equal 1 $envReg.Count 'CLAUDE_AUTO_PROJECTS_ROOT overrides the default -ProjectsRoot when the caller omits it'
    Assert-Equal 'EnvA' $envReg[0].Slug 'and it is the fixture project, not whatever the real machine has'
} finally {
    if ($null -eq $savedEnvRoot) { Remove-Item Env:CLAUDE_AUTO_PROJECTS_ROOT -ErrorAction SilentlyContinue }
    else { $env:CLAUDE_AUTO_PROJECTS_ROOT = $savedEnvRoot }
    Remove-Item -LiteralPath $envRoot -Recurse -Force -ErrorAction SilentlyContinue
}
# An explicit -ProjectsRoot still wins over the environment variable - the parameter is the more
# specific instruction, exactly like every other overridable default in this codebase. Proven by
# pointing the ENV VAR at a root that does not exist at all (Get-ProjectRegistry returns @() for
# that immediately) while the explicit argument names a real fixture - if the env var silently won,
# this would come back empty instead.
$explicitRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-explicitroot-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-ProjFixture -Root $explicitRoot -Slug 'ExplicitWins' -RealPath $here -When (Get-Date) | Out-Null
$savedEnvRoot2 = $env:CLAUDE_AUTO_PROJECTS_ROOT
try {
    $env:CLAUDE_AUTO_PROJECTS_ROOT = 'C:\this-env-value-must-be-ignored-and-does-not-exist'
    $explicitCache = Join-Path $explicitRoot 'cache-explicit.json'
    $explicitReg = @(Get-ProjectRegistry -ProjectsRoot $explicitRoot -CachePath $explicitCache)
    Assert-Equal 1 $explicitReg.Count 'an explicit -ProjectsRoot argument still wins over the environment variable'
    Assert-Equal 'ExplicitWins' $explicitReg[0].Slug 'and it is the explicitly named root''s project'
} finally {
    if ($null -eq $savedEnvRoot2) { Remove-Item Env:CLAUDE_AUTO_PROJECTS_ROOT -ErrorAction SilentlyContinue }
    else { $env:CLAUDE_AUTO_PROJECTS_ROOT = $savedEnvRoot2 }
    Remove-Item -LiteralPath $explicitRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$ps = @(
  [pscustomobject]@{ Slug='A'; Path='C:\w\alpha';  Name='alpha';  Worktree=$null }
  [pscustomobject]@{ Slug='B'; Path='C:\w\beta';   Name='beta';   Worktree=$null }
)
Assert-Equal 1 (@(Select-ProjectMatch -Projects $ps -Filter 'al').Count) 'filter matches the name'
Assert-Equal 2 (@(Select-ProjectMatch -Projects $ps -Filter 'w\').Count) 'filter matches the path'
Assert-Equal 2 (@(Select-ProjectMatch -Projects $ps -Filter '').Count) 'empty filter passes everything'
# A pasted literal path must match its own row and only its own row - -like, not regex, is what
# makes this true: 'C:\w\alpha' would be an escape-laden pattern under -match.
Assert-Equal 1 (@(Select-ProjectMatch -Projects $ps -Filter 'C:\w\alpha').Count) 'a pasted literal path matches its own row'

# '[' is a character-class opener to -like; an unescaped, unmatched one is a TERMINATING
# WildcardPatternException that would take the render loop down. The filter text must be escaped,
# not merely caught, so someone who typed '[' looking for a literal bracket does not get an
# unexplained empty list.
$threwBracket = $false
try { $bracketMatches = @(Select-ProjectMatch -Projects $ps -Filter '[') } catch { $threwBracket = $true }
Assert-Equal $false $threwBracket 'a filter of "[" does not throw'
Assert-Equal 0 $bracketMatches.Count 'and matches nothing - neither row contains a literal bracket'

$r = Resolve-StartProject -Cwd 'C:\w\alpha' -Remembered 'C:\w\beta' -Projects $ps
Assert-Equal 'cwd' $r.Source 'a cwd that is a known project wins'
Assert-Equal 'C:\w\alpha' $r.Path 'and it is the chosen path'

# ConvertTo-ProjectKey (fix round 2, IMPORTANT 3): the one shared normaliser Resolve-StartProject,
# Invoke-ProjectScreen's slug lookup and its -Initial match all now go through.
Assert-Equal 'c:\w\alpha' (ConvertTo-ProjectKey 'C:\W\Alpha\') 'ConvertTo-ProjectKey trims a trailing separator and lowercases'
Assert-Equal (ConvertTo-ProjectKey 'C:\w\alpha') (ConvertTo-ProjectKey 'c:/w/alpha/') 'a forward-slash, trailing-slash spelling normalises to the same key'
Assert-Equal '' (ConvertTo-ProjectKey '') 'an empty path normalises to an empty key, not a lone separator'

# Resolve-StartProject inherits the slash-direction fix for free: it used to compare with its own
# inline TrimEnd/ToLower, which never translated '/' to '\' - a forward-slash cwd spelling silently
# fell through to the 'none'/'remembered' branch. It now goes through ConvertTo-ProjectKey too.
$r5 = Resolve-StartProject -Cwd 'c:/w/alpha/' -Remembered '' -Projects $ps
Assert-Equal 'cwd' $r5.Source 'a forward-slash, trailing-slash cwd spelling still matches the known project'

# The remembered branch now checks disk, so it needs a REAL directory - a raw prefs string that
# never passed through the registry must not be handed back if it no longer exists.
$rememberedDir = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-remembered-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $rememberedDir | Out-Null

$r2 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered $rememberedDir -Projects $ps
Assert-Equal 'remembered' $r2.Source 'an unknown cwd falls back to the remembered project'

$r3 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered '' -Projects $ps
Assert-Equal 'none' $r3.Source 'nothing known and nothing remembered is unresolved'

$vanishedDir = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-vanished-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $vanishedDir | Out-Null
Remove-Item -LiteralPath $vanishedDir -Recurse -Force
$r4 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered $vanishedDir -Projects $ps
Assert-Equal 'none' $r4.Source 'a remembered project that has vanished from disk is not offered'
Remove-Item -LiteralPath $rememberedDir -Recurse -Force -ErrorAction SilentlyContinue

# --- Set-LaunchStartProject (Task 9): the wiring claude-auto.ps1 calls, given a unit seam since the
# decision loop itself has no console. ---
$slsProjects = @(
  [pscustomobject]@{ Slug='SLS-A'; Path='C:\w\alpha'; Name='alpha'; Worktree=$null }
)
$slsState = [pscustomobject]@{ Project = ''; ProjectSlug = 'stale' }
$slsSource = Set-LaunchStartProject -State $slsState -Cwd 'C:\w\alpha' -Projects $slsProjects
Assert-Equal 'cwd' $slsSource 'Set-LaunchStartProject returns the Source Resolve-StartProject picked'
Assert-Equal 'C:\w\alpha' $slsState.Project 'and applies the Path onto State.Project'
Assert-Equal 'SLS-A' $slsState.ProjectSlug 'and looks the slug up in the registry, replacing whatever was there before'

$slsState2 = [pscustomobject]@{ Project = ''; ProjectSlug = '' }
$slsSource2 = Set-LaunchStartProject -State $slsState2 -Cwd 'C:\Users\someone\Desktop' -Projects $slsProjects
Assert-Equal 'none' $slsSource2 'an unresolved cwd (no cwd match, nothing remembered) reports Source none'
Assert-Equal '' "$($slsState2.Project)" 'and leaves State.Project empty rather than a $null that later string ops would choke on'
Assert-Equal '' $slsState2.ProjectSlug 'and no slug either'

# A resolved path outside the registry (the remembered branch, or a cwd matched only by holding a
# .git) carries no slug - the registry lookup must not silently invent one.
$gitCwd = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-set-start-git-$([Guid]::NewGuid().ToString('N'))")
New-Item -ItemType Directory -Path (Join-Path $gitCwd '.git') -Force | Out-Null
try {
    $slsState3 = [pscustomobject]@{ Project = ''; ProjectSlug = '' }
    $slsSource3 = Set-LaunchStartProject -State $slsState3 -Cwd $gitCwd -Projects $slsProjects
    Assert-Equal 'cwd' $slsSource3 'a cwd holding a .git resolves via Source cwd even when it is not a registered project'
    Assert-Equal $gitCwd $slsState3.Project 'and its own path is applied'
    Assert-Equal '' $slsState3.ProjectSlug 'but it carries no slug - it is not in the registry'
} finally { Remove-Item -LiteralPath $gitCwd -Recurse -Force -ErrorAction SilentlyContinue }

# --- one real directory is ONE row -----------------------------------------------------------------
# Two slug folders can name the same directory: a cwd recorded with different separators, a folder
# renamed and renamed back. A row per SLUG put two rows with identical Name and indistinguishable
# Path columns on the screen, and every slug lookup took $hit[0] - so half of that project's sessions
# could not be reached from the screen that had just named it (adversarial review 2026-09-16, A12).
$mgRoot = Join-Path $env:TEMP ('claude-auto-merge-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$mgReal = Join-Path $mgRoot 'Shared'
$mgProjects = Join-Path $mgRoot 'projects'
New-Item -ItemType Directory -Force -Path $mgReal | Out-Null
$mgWrite = {
    param([string]$Slug, [string]$Cwd, [datetime]$When)
    $d = Join-Path $mgProjects $Slug
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    $f = Join-Path $d "$Slug.jsonl"
    [IO.File]::WriteAllText($f, '{"type":"user","cwd":' + (ConvertTo-Json $Cwd) + ',"message":{"role":"user","content":"a prompt"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
    (Get-Item -LiteralPath $f).LastWriteTime = $When
}
& $mgWrite 'C--tmp-Shared' $mgReal (Get-Date '2026-09-01 12:00')
& $mgWrite 'C--tmp-Shared-alt' ($mgReal.Replace('\', '/')) (Get-Date '2026-09-01 11:00')
$mgReg = @(Get-ProjectRegistry -ProjectsRoot $mgProjects -CachePath (Join-Path $mgRoot 'c.json'))
Assert-Equal 1 $mgReg.Count 'two slug folders naming one directory are ONE row on the project screen'
Assert-Equal 'C--tmp-Shared' $mgReg[0].Slug 'whose primary slug is the one with the newest activity'
Assert-Equal 'C--tmp-Shared,C--tmp-Shared-alt' ((@($mgReg[0].Slugs) | Sort-Object) -join ',') 'and which keeps EVERY slug of that directory, because that is what the session picker scopes on'
Assert-Equal (Get-Date '2026-09-01 12:00') $mgReg[0].LastActivity 'with the newest activity of the two'
$mgState = [pscustomobject]@{ Project = ''; ProjectSlug = ''; ProjectSlugs = @() }
$null = Set-LaunchStartProject -State $mgState -Cwd $mgReal -Projects $mgReg
Assert-Equal 'C--tmp-Shared,C--tmp-Shared-alt' ((@($mgState.ProjectSlugs) | Sort-Object) -join ',') 'and the launch state carries the whole set into the session picker, not just the first slug'
$script:LaunchStartContext = $null
Remove-Item -LiteralPath $mgRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- a project at a drive root ----------------------------------------------------------------------
# Split-Path -Leaf 'C:\' returns 'C:\', so the Name and Path columns rendered the identical string
# (adversarial review 2026-09-16, A1). Asserted through the same helper the registry uses, against a
# fixture drive-root SHAPE rather than the real C:\.
$drRoot = Join-Path $env:TEMP ('claude-auto-drive-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$drProjects = Join-Path $drRoot 'projects'
$drSlug = Join-Path $drProjects 'C--'
New-Item -ItemType Directory -Force -Path $drSlug | Out-Null
$drDrive = (Split-Path -Path $drRoot -Qualifier) + '\'
[IO.File]::WriteAllText((Join-Path $drSlug 'd1.jsonl'), '{"type":"user","cwd":' + (ConvertTo-Json $drDrive) + ',"message":{"role":"user","content":"a prompt"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
$drReg = @(Get-ProjectRegistry -ProjectsRoot $drProjects -CachePath (Join-Path $drRoot 'c.json'))
Assert-Equal 1 $drReg.Count 'a drive-root cwd is still a project row'
Assert-Equal $drDrive $drReg[0].Path 'whose Path is the drive root exactly as it was recorded'
Assert-Equal ($drDrive.TrimEnd('\')) $drReg[0].Name 'and whose NAME is a name, not the whole path again'
Remove-Item -LiteralPath $drRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- a cwd carrying a NUL is dropped QUIETLY ---------------------------------------------------------
# Test-Path raises a non-terminating ArgumentException for an embedded NUL instead of answering
# $false, and at the default $ErrorActionPreference that is a four-line red dump over the screen.
$nulRoot = Join-Path $env:TEMP ('claude-auto-nul-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$nulProjects = Join-Path $nulRoot 'projects'
$nulSlug = Join-Path $nulProjects 'C--src-nul'
New-Item -ItemType Directory -Force -Path $nulSlug | Out-Null
[IO.File]::WriteAllText((Join-Path $nulSlug 'n1.jsonl'), '{"type":"user","cwd":"C:\\src\\' + [char]92 + 'u0000bad","message":{"role":"user","content":"a prompt"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
$nulOut = @(Get-ProjectRegistry -ProjectsRoot $nulProjects -CachePath (Join-Path $nulRoot 'c.json') 2>&1)
Assert-Equal 0 @($nulOut | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] }).Count 'a cwd carrying a NUL produces no PowerShell error record on the screen'
Assert-Equal 0 @($nulOut | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] }).Count 'and the row it could never have entered is dropped'
Remove-Item -LiteralPath $nulRoot -Recurse -Force -ErrorAction SilentlyContinue

if ($script:Ran -ne 50) { Write-Host "COULD NOT RUN: expected 50 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
