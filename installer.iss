[Setup]
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName=H2AC-RS
AppVersion=1.0.0
AppPublisher=H2AC-RS
DefaultDirName={autopf}\H2AC-RS
DefaultGroupName=H2AC-RS
OutputDir=.
OutputBaseFilename=h2ac-rs-setup
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
Source: "target\dist\h2ac-rs\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\H2AC-RS"; Filename: "{app}\h2ac-rs.exe"
Name: "{group}\卸载 H2AC-RS"; Filename: "{uninstallexe}"
Name: "{commondesktop}\H2AC-RS"; Filename: "{app}\h2ac-rs.exe"

[Run]
Filename: "{app}\h2ac-rs.exe"; Description: "启动 H2AC-RS"; Flags: nowait postinstall skipifsilent

[Code]
function InitializeSetup: Boolean;
begin
  if not IsAdminLoggedOn then
  begin
    MsgBox('建议以管理员身份安装以确保游戏内宏功能正常。', mbInformation, MB_OK);
  end;
  Result := True;
end;
