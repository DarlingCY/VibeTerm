#define MyAppName "VibeTerm"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#ifndef MySourceDir
  #define MySourceDir "."
#endif
#ifndef MyOutputDir
  #define MyOutputDir AddBackslash(MySourceDir) + "dist"
#endif
#define MyAppPublisher "DarlingCY"
#define MyAppURL "https://github.com/DarlingCY/VibeTerm"
#define MyAppExeName "vibeterm.exe"

[Setup]
AppId={{A9C1283A-EC7F-4F4E-8A39-6F5E4A3B9A11}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
SetupIconFile={#MySourceDir}\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
LicenseFile={#MySourceDir}\LICENSE
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
OutputDir={#MyOutputDir}
OutputBaseFilename=vibeterm-setup-{#MyAppVersion}-windows-x64

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#MySourceDir}\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MySourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MySourceDir}\scripts\install-context-menu.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "{#MySourceDir}\scripts\uninstall-context-menu.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\install-context-menu.ps1"" -ExePath ""{app}\{#MyAppExeName}"""; Flags: runhidden waituntilterminated; Check: PowerShellExists
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\uninstall-context-menu.ps1"""; Flags: runhidden waituntilterminated; Check: PowerShellExists

[Code]
function PowerShellExists(): Boolean;
begin
  Result := FileExists(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'));
end;
