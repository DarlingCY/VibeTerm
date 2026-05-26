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

Write-Host "Removed Windows context menu for VibeTerm"
