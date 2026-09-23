# Scripted capture of every figure the README references.
#
# Each run opens a window, ticks the simulation deterministically (the tick
# schedule is independent of wall-clock time), grabs one frame and exits. All of
# the behaviour is driven by FORMICA_* environment variables, so the figures are
# reproducible rather than hand-taken.

[CmdletBinding()]
param(
    # Figures live in their own subdirectory, one level under docs.
    [string]$OutDir = "docs/figures"
)

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

# Every FORMICA_* switch the app understands. Listed explicitly rather than
# enumerated from `Env:`: on Windows PowerShell 5.1, `Get-ChildItem Env:` throws
# "An item with the same key has already been added" whenever the environment
# contains two names differing only in case, which is common on Windows
# (`Path` and `PATH`).
$FormicaVars = @(
    "FORMICA_TICKS_PER_FRAME", "FORMICA_SCREENSHOT", "FORMICA_SCREENSHOT_FRAME",
    "FORMICA_EXIT_AFTER", "FORMICA_SHOW_FOOD", "FORMICA_SHOW_HOME",
    "FORMICA_FOOD_GAIN", "FORMICA_HOME_GAIN", "FORMICA_COLONIES",
    "FORMICA_PREDATORS", "FORMICA_ANTS", "FORMICA_MAP_SIZE",
    "FORMICA_OBSTACLES", "FORMICA_BLOCK_DENSITY", "FORMICA_EVAPORATION",
    "FORMICA_INDIVIDUALITY", "FORMICA_CROWD_THRESHOLD", "FORMICA_CROWD_RELIEF",
    "FORMICA_U_WALL_GAP", "FORMICA_U_WALL_ARM"
)

function Clear-FormicaEnv {
    foreach ($name in $FormicaVars) {
        if (Test-Path "Env:$name") { Remove-Item "Env:$name" }
    }
}

function Capture {
    param(
        [string]$Name,
        [hashtable]$Env,
        [int]$TicksPerFrame = 1,
        [int]$Frame = 300
    )
    Clear-FormicaEnv
    $env:FORMICA_TICKS_PER_FRAME = "$TicksPerFrame"
    $env:FORMICA_SCREENSHOT_FRAME = "$Frame"
    $env:FORMICA_EXIT_AFTER = "$($Frame + 40)"
    $env:FORMICA_SCREENSHOT = (Join-Path $root "$OutDir\$Name.png")
    foreach ($key in $Env.Keys) { Set-Item "Env:$key" $Env[$key] }

    Write-Host "capturing $Name (frame $Frame @ $TicksPerFrame ticks/frame, tick $($Frame * $TicksPerFrame))..."
    # Redirect to a file rather than piping: PowerShell 5.1 turns a native
    # program's stderr into a terminating `NativeCommandError` the moment it
    # reaches a pipeline, and Bevy logs to stderr.
    $log = Join-Path $root "target\capture-$Name.log"
    cargo run -q -p formica-app --release *> $log
    if ($LASTEXITCODE -ne 0) { throw "cargo run failed for $Name (see $log)" }
    Select-String -Path $log -Pattern "captured frame|panic" |
        ForEach-Object { Write-Host "  $($_.Line)" }
    if (-not (Test-Path (Join-Path $root "$OutDir\$Name.png"))) {
        throw "$Name produced no PNG (see $log)"
    }
}

# --- Phase 1/2: the individual and the emergent structure -------------------
Capture "fix1-ring-identical-ants" @{ FORMICA_INDIVIDUALITY = "0" } 1 30
Capture "fix1-ring-individuals"    @{}                              1 30
Capture "phase2-early"             @{}                              1 60
Capture "phase2-converged"         @{}                              4 250

# --- fix 2: food piles shrink, then disappear ------------------------------
Capture "fix2-food-shrinking"      @{}                              4 450
Capture "fix2-food-exhausted"      @{}                              8 560

# --- Phase 4: the optional features ----------------------------------------
Capture "p4-u-wall"        @{ FORMICA_OBSTACLES = "u-wall" }                       4 750
Capture "p4-predators"     @{ FORMICA_PREDATORS = "5" }                            4 625
Capture "p4-two-colonies"  @{ FORMICA_COLONIES = "2" }                             4 750
Capture "p4-everything"    @{ FORMICA_COLONIES = "2"; FORMICA_PREDATORS = "5";
                              FORMICA_OBSTACLES = "blocks" }                       4 600

# --- fix 3: a dense maze, with and without crowding relief -----------------
$labyrinth = @{ FORMICA_OBSTACLES = "blocks"; FORMICA_BLOCK_DENSITY = "0.45" }
Capture "fix3-labyrinth-collapsed" ($labyrinth + @{ FORMICA_CROWD_RELIEF = "0" })  4 700
Capture "fix3-labyrinth-spread"    $labyrinth                                      4 700

Clear-FormicaEnv
Pop-Location
Get-ChildItem (Join-Path $root $OutDir) -Filter *.png | Select-Object Name, Length
