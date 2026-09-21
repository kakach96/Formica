#!/usr/bin/env pwsh
# Re-measure every number the README quotes. Run with the release profile.
#
#   pwsh -File scripts/measure-readme.ps1
#
# Output goes to target/measure.txt as well as the console.

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
$log = Join-Path $root "target\measure.txt"
if (Test-Path $log) { Remove-Item $log }

function Section($title) {
    Write-Host "`n=== $title ==="
    Add-Content $log "`n=== $title ==="
}

# NOTE: the parameter must not be called `$Args` - that is a PowerShell
# automatic variable, and `@Args` would silently expand to the (empty) automatic
# one, running every measurement with no arguments at all.
function Run($title, [string[]]$CliArgs) {
    Section $title
    Write-Host "formica $($CliArgs -join ' ')"
    $out = Join-Path $root "target\measure-tmp.txt"
    cargo run -q -p formica-headless --release -- @CliArgs *> $out
    Get-Content $out | Tee-Object -FilePath $log -Append | Out-Null
    Get-Content $out | Select-String -Pattern "delivered |recovery |sigmoid|concentration|VERDICT|first delivery|10% |50% |90% |peak delivery|round trips|^\s+\d" | ForEach-Object { Write-Host $_.Line }
}

# --- headline acceptance ----------------------------------------------------
Run "Phase 1 acceptance, default config, 5000 ticks" @("--ticks", "5000")

# --- Phase 4 comparisons ----------------------------------------------------
Run "U-wall, 96x96 headroom, 2500 ticks, no wall" @("--width","96","--height","96","--ants","140","--food-per-pile","4000","--ticks","2500","--quiet")
Run "U-wall, 96x96 headroom, 2500 ticks, U-wall" @("--width","96","--height","96","--ants","140","--food-per-pile","4000","--ticks","2500","--quiet","--obstacles","u-wall")
Run "Predator sweep, 256x256, 2500 ticks" @("--sweep-predators","0,1,2,4,8","--ticks","2500")

# --- sweeps -----------------------------------------------------------------
Run "Evaporation sweep, default scale, 5000 ticks" @("--sweep-evaporation","0.005,0.01,0.03,0.05,0.1,0.2,0.3","--ticks","5000")
Run "Evaporation sweep, sparse (60 ants), 8000 ticks" @("--sweep-evaporation","0.005,0.01,0.02,0.05,0.1,0.2,0.3","--ants","60","--ticks","8000","--food-per-pile","2500")
Run "Random-walk sweep, sparse (60 ants), 8000 ticks" @("--sweep-random-walk","0.0,0.1,0.3,0.6,1.0,1.5","--ants","60","--ticks","8000","--food-per-pile","2500")
Run "Individuality sweep, default scale, 5000 ticks" @("--sweep-individuality","0.0,0.2,0.4,0.6,1.0,1.5","--ticks","5000")
Run "Individuality sweep, sparse (60 ants), 8000 ticks" @("--sweep-individuality","0.0,0.2,0.4,0.6,1.0,1.5","--ants","60","--ticks","8000","--food-per-pile","2500")

# --- crowding relief --------------------------------------------------------
Section "Crowding: dense labyrinth, relief off vs on"
foreach ($density in @(0.15, 0.25, 0.35, 0.45)) {
    foreach ($relief in @(0.0, 0.6)) {
        $csv = Join-Path $root "target\crowd-$density-$relief.csv"
        cargo run -q -p formica-headless --release -- --ticks 3000 --quiet --report-every 0 `
            --sample-interval 1 --csv $csv --obstacles blocks --block-density $density `
            --crowd-relief $relief *> (Join-Path $root "target\crowd-tmp.txt")
        $rows = Get-Content $csv | Select-Object -Skip 1
        $warm = $rows | Where-Object { [int](($_ -split ',')[0]) -gt 200 }
        $vals = $warm | ForEach-Object { [int]($_ -split ',')[11] }
        $line = "density {0,-5} relief {1,-4} peak bin {2,4}  mean {3,6}  delivered {4}" -f `
            $density, $relief, ($vals | Measure-Object -Maximum).Maximum,
            [math]::Round(($vals | Measure-Object -Average).Average, 1),
            (($rows | Select-Object -Last 1) -split ',')[1]
        Write-Host $line
        Add-Content $log $line
    }
}

Pop-Location
Write-Host "`nwrote $log"
