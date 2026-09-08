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

Assert-Equal (Get-ProjectPathFromTranscript -Directory $dir) $real 'cwd comes from the transcript'
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

Assert-Equal $reg.Count 2 'only resolvable, existing projects are listed'
Assert-Equal $reg[0].Slug 'A' 'newest activity first'
Assert-Equal $reg[1].Slug 'B' 'older second'
Assert-Equal $reg[0].Path $here 'path comes from the transcript'
Assert-True (Test-Path -LiteralPath $cache) 'cache file written'

# Deleting the underlying jsonl removes the directory's newest-file entirely, so the loop's own
# 'if (-not $newest) { continue }' drops the project before the cache is ever consulted - this
# exercises THAT guard, not the cache. The cache itself is proven by the two cases below.
Remove-Item -LiteralPath (Join-Path $root2 'B\s.jsonl') -Force
$reg2 = @(Get-ProjectRegistry -ProjectsRoot $root2 -CachePath $cache)
Assert-Equal (@($reg2 | Where-Object { $_.Slug -eq 'B' }).Count) 0 'a project with no transcript left drops out'
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
Assert-Equal ($reg3a | Where-Object { $_.Slug -eq 'Hit' }).Path $hitPath1 'first call resolves the path from the transcript'

$hitFile = Join-Path $hitDir 's.jsonl'
$hitWhen = (Get-Item -LiteralPath $hitFile).LastWriteTime
Set-Content -LiteralPath $hitFile -Encoding utf8 -NoNewline -Value (
    @{ type='user'; cwd=$hitPath2 } | ConvertTo-Json -Compress)
(Get-Item -LiteralPath $hitFile).LastWriteTime = $hitWhen   # same key: the cache must answer, not the new content

$reg3b = @(Get-ProjectRegistry -ProjectsRoot $root3 -CachePath $cache3)
Assert-Equal ($reg3b | Where-Object { $_.Slug -eq 'Hit' }).Path $hitPath1 'a cache hit (same mtime) returns the cached path, not a re-read of the changed transcript'
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

$ps = @(
  [pscustomobject]@{ Slug='A'; Path='C:\w\alpha';  Name='alpha';  Worktree=$null }
  [pscustomobject]@{ Slug='B'; Path='C:\w\beta';   Name='beta';   Worktree=$null }
)
Assert-Equal (@(Select-ProjectMatch -Projects $ps -Filter 'al').Count) 1 'filter matches the name'
Assert-Equal (@(Select-ProjectMatch -Projects $ps -Filter 'w\').Count) 2 'filter matches the path'
Assert-Equal (@(Select-ProjectMatch -Projects $ps -Filter '').Count) 2 'empty filter passes everything'
# A pasted literal path must match its own row and only its own row - -like, not regex, is what
# makes this true: 'C:\w\alpha' would be an escape-laden pattern under -match.
Assert-Equal (@(Select-ProjectMatch -Projects $ps -Filter 'C:\w\alpha').Count) 1 'a pasted literal path matches its own row'

# '[' is a character-class opener to -like; an unescaped, unmatched one is a TERMINATING
# WildcardPatternException that would take the render loop down. The filter text must be escaped,
# not merely caught, so someone who typed '[' looking for a literal bracket does not get an
# unexplained empty list.
$threwBracket = $false
try { $bracketMatches = @(Select-ProjectMatch -Projects $ps -Filter '[') } catch { $threwBracket = $true }
Assert-Equal $false $threwBracket 'a filter of "[" does not throw'
Assert-Equal 0 $bracketMatches.Count 'and matches nothing - neither row contains a literal bracket'

$r = Resolve-StartProject -Cwd 'C:\w\alpha' -Remembered 'C:\w\beta' -Projects $ps
Assert-Equal $r.Source 'cwd' 'a cwd that is a known project wins'
Assert-Equal $r.Path 'C:\w\alpha' 'and it is the chosen path'

# The remembered branch now checks disk, so it needs a REAL directory - a raw prefs string that
# never passed through the registry must not be handed back if it no longer exists.
$rememberedDir = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-remembered-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $rememberedDir | Out-Null

$r2 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered $rememberedDir -Projects $ps
Assert-Equal $r2.Source 'remembered' 'an unknown cwd falls back to the remembered project'

$r3 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered '' -Projects $ps
Assert-Equal $r3.Source 'none' 'nothing known and nothing remembered is unresolved'

$vanishedDir = Join-Path ([IO.Path]::GetTempPath()) ("cap-proj-vanished-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $vanishedDir | Out-Null
Remove-Item -LiteralPath $vanishedDir -Recurse -Force
$r4 = Resolve-StartProject -Cwd 'C:\Users\someone\Desktop' -Remembered $vanishedDir -Projects $ps
Assert-Equal $r4.Source 'none' 'a remembered project that has vanished from disk is not offered'
Remove-Item -LiteralPath $rememberedDir -Recurse -Force -ErrorAction SilentlyContinue

if ($script:Ran -ne 23) { Write-Host "COULD NOT RUN: expected 23 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
