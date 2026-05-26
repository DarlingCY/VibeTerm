$ErrorActionPreference = "Stop"

$keys = @(
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm.AddPane",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm.NewTab",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm.AddPane",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm.NewTab",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTermNewTab",
    "HKCU:\Software\Classes\Directory\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\Directory\shell\VibeTermNewTab",
    "HKCU:\Software\Classes\Folder\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\Folder\shell\VibeTermNewTab",
    "HKCU:\Software\Classes\Drive\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\Drive\shell\VibeTermNewTab",
    "HKCU:\Software\Classes\DesktopBackground\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\DesktopBackground\shell\VibeTermNewTab"
)

foreach ($key in $keys) {
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Recurse -Force
    }
}

$doubleClickShellRoots = @(
    "HKCU:\Software\Classes\Directory\shell",
    "HKCU:\Software\Classes\Folder\shell",
    "HKCU:\Software\Classes\Drive\shell"
)

foreach ($root in $doubleClickShellRoots) {
    New-Item -Path $root -Force | Out-Null
    Set-Item -LiteralPath $root -Value "none"
}

Write-Host "Removed Windows context menu for VibeTerm"
