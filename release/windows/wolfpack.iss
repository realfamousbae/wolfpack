; The Windows installer of wolf (Inno Setup 6). Built by the release workflow:
;   iscc /DVersion=0.1.0 /DSource=<directory with wolf.exe> /DOutput=<directory> wolfpack.iss
; It installs for the current user without administrator rights, puts wolf on the user's PATH and
; runs `wolf setup` as its last step (spec chapter 8, section 2a, rule 5).

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{8C1B5E2A-4F7D-4E0B-9B5C-2D6A1E7F3C40}
AppName=wolfpack
AppVersion={#Version}
AppPublisher=The wolfpack contributors
AppPublisherURL=https://github.com/realfamousbae/wolfpack
DefaultDirName={localappdata}\Programs\wolfpack
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
OutputDir={#Output}
OutputBaseFilename=wolfpack-{#Version}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile={#Source}\LICENSE-MIT

[Files]
Source: "{#Source}\wolf.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#Source}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Run]
Filename: "{app}\bin\wolf.exe"; Parameters: "setup"; \
  StatusMsg: "Installing Node.js, TypeScript, Go and Rust for wolf (about 1 GB)..."; \
  Flags: waituntilterminated

[Messages]
FinishedLabel=wolf is installed. Open a new terminal (PowerShell or cmd) and run: wolf new hello, cd hello, wolf run. If a download failed, run wolf setup again.

[Code]
const
  EnvKey = 'Environment';

function BinDir(): string;
begin
  Result := ExpandConstant('{app}\bin');
end;

{ Adds the bin directory to the user's PATH once. }
procedure AddToPath();
var
  Path: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    Path := '';
  if Pos(';' + Uppercase(BinDir()) + ';', ';' + Uppercase(Path) + ';') > 0 then
    exit;
  if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then
    Path := Path + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path + BinDir());
end;

{ Removes the bin directory from the user's PATH. }
procedure RemoveFromPath();
var
  Path: string;
  Index: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    exit;
  Path := ';' + Path + ';';
  Index := Pos(';' + Uppercase(BinDir()) + ';', Uppercase(Path));
  if Index = 0 then
    exit;
  Delete(Path, Index, Length(BinDir()) + 1);
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddToPath();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath();
end;
