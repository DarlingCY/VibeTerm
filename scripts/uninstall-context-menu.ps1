$ErrorActionPreference = "Stop"

$keys = @(
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm.AddPane",
    "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm.NewTab",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm.AddPane",
    "HKCU:\Software\Classes\Directory\shell\VibeTerm.NewTab"
)

foreach ($key in $keys) {
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Recurse -Force
    }
}

Write-Host "Removed Windows context menu for VibeTerm"
