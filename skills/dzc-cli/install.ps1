# Junction dzc-cli skill to $env:USERPROFILE\.claude\skills\dzc-cli
$ErrorActionPreference = "Stop"
$SkillDir = (Resolve-Path "$PSScriptRoot").Path
$Target = Join-Path $env:USERPROFILE ".claude\skills\dzc-cli"

if ($args[0] -eq "--uninstall") {
    if (Test-Path $Target) {
        cmd /c rmdir $Target
        Write-Host "Uninstalled: $Target"
    }
    exit 0
}

New-Item -ItemType Directory -Force -Path (Split-Path $Target) | Out-Null
if (Test-Path $Target) { cmd /c rmdir $Target }
New-Item -ItemType Junction -Path $Target -Target $SkillDir | Out-Null
Write-Host "Installed: $Target -> $SkillDir"
Write-Host "Reload any active Claude sessions to pick up the new skill."
