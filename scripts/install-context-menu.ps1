param(
    [string]$ExePath = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $ExePath = Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..\target\release")).Path "vibeterm.exe"
}

if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "vibeterm.exe not found: $ExePath. Run `cargo build --release` first, or pass -ExePath."
}

$ExePath = (Resolve-Path -LiteralPath $ExePath).Path
$command = '"{0}" "%V"' -f $ExePath
$backgroundKey = "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm"
$directoryKey = "HKCU:\Software\Classes\Directory\shell\VibeTerm"

foreach ($key in @($backgroundKey, $directoryKey)) {
    New-Item -Path $key -Force | Out-Null
    New-ItemProperty -Path $key -Name "MUIVerb" -Value "Open with VibeTerm" -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $key -Name "Icon" -Value $ExePath -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $key -Name "Position" -Value "Top" -PropertyType String -Force | Out-Null

    $commandKey = Join-Path $key "command"
    New-Item -Path $commandKey -Force | Out-Null
    Set-ItemProperty -Path $commandKey -Name "(default)" -Value $command
}

Write-Host "Installed Windows context menu: Open with VibeTerm"
Write-Host "Executable: $ExePath"
