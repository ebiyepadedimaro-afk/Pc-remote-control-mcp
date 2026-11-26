; PC Remote Control — Inno Setup installer (built in CI on windows-latest)
#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\pkg\win\PC Remote Control"
#endif
#ifndef IconFile
  #define IconFile "..\..\ui\assets\icon.ico"
#endif
#ifndef OutDir
  #define OutDir "..\..\out"
#endif

[Setup]
AppId={{47FD5BCB-D68F-46E9-AB01-52D5432DBD09}
AppName=PC Remote Control
AppVersion={#MyAppVersion}
AppPublisher=PC Remote Control contributors
AppPublisherURL=https://github.com/ebiyepadedimaro-afk/Pc-remote-control-mcp
AppSupportURL=https://github.com/ebiyepadedimaro-afk/Pc-remote-control-mcp/issues
DefaultDirName={autopf}\PC Remote Control
DefaultGroupName=PC Remote Control
DisableProgramGroupPage=yes
OutputDir={#OutDir}
OutputBaseFilename=PC-Remote-Control-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\PCRemoteControl.exe
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"

[Icons]
Name: "{autoprograms}\PC Remote Control"; Filename: "{app}\PCRemoteControl.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\PC Remote Control"; Filename: "{app}\PCRemoteControl.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\PCRemoteControl.exe"; Description: "Launch PC Remote Control"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath('{app}'); Flags: preservestringtype

[Code]
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + ExpandConstant(Param) + ';', ';' + OrigPath + ';') = 0;
end;
