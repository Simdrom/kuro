; Installer for one user, without administrator rights (Inno Setup 6).
;   iscc /DAppVersion=0.2.0 /DFiles=<folder with kuro.exe and the licences> kuro.iss
; "Start with Windows" and kuro:// links belong to Kuro's own settings: they are only
; cleaned up here. Lists, settings and sessions are never touched.

#ifndef AppVersion
  #error Pass /DAppVersion=X.Y.Z
#endif
#ifndef Files
  #error Pass /DFiles=<folder>
#endif

[Setup]
; Identifies Kuro to Windows. Must never change.
AppId={{430942B2-A6DE-46B7-96A7-DDF405974F13}
AppName=Kuro
AppVersion={#AppVersion}
AppVerName=Kuro {#AppVersion}
AppPublisher=Simdrom
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={autopf}\Kuro
DisableDirPage=auto
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=..\..\crates\app\assets\app.ico
UninstallDisplayIcon={app}\kuro.exe
UninstallDisplayName=Kuro
ShowLanguageDialog=auto
OutputBaseFilename=kuro-{#AppVersion}-windows-x64-setup
; Kuro is asked to quit first; this is for a copy that does not answer.
CloseApplications=force
RestartApplications=no
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "nb"; MessagesFile: "compiler:Languages\Norwegian.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"

[CustomMessages]
en.Updating=Kuro %1 is installed. It will be updated to %2.
en.UpdateButton=&Update
en.SameVersion=Kuro %1 is already installed. Install it again?
en.NewerInstalled=Kuro %1 is installed, which is newer than this installer (%2). An older Kuro cannot open the lists of a newer one, so nothing has been changed.
es.Updating=Kuro %1 está instalado. Se actualizará a %2.
es.UpdateButton=&Actualizar
es.SameVersion=Kuro %1 ya está instalado. ¿Instalarlo de nuevo?
es.NewerInstalled=Kuro %1 está instalado y es más nuevo que este instalador (%2). Un Kuro anterior no puede abrir las listas de uno más nuevo, así que no se ha cambiado nada.
fr.Updating=Kuro %1 est installé. Il sera mis à jour vers la version %2.
fr.UpdateButton=&Mettre à jour
fr.SameVersion=Kuro %1 est déjà installé. L'installer à nouveau ?
fr.NewerInstalled=Kuro %1 est installé et plus récent que ce programme d'installation (%2). Un Kuro plus ancien ne peut pas ouvrir les listes d'une version plus récente : rien n'a été modifié.
de.Updating=Kuro %1 ist installiert. Es wird auf %2 aktualisiert.
de.UpdateButton=&Aktualisieren
de.SameVersion=Kuro %1 ist bereits installiert. Erneut installieren?
de.NewerInstalled=Kuro %1 ist installiert und neuer als dieses Installationsprogramm (%2). Ein älteres Kuro kann die Listen eines neueren nicht öffnen, daher wurde nichts geändert.
it.Updating=Kuro %1 è installato. Verrà aggiornato alla versione %2.
it.UpdateButton=&Aggiorna
it.SameVersion=Kuro %1 è già installato. Installarlo di nuovo?
it.NewerInstalled=Kuro %1 è installato ed è più recente di questo programma di installazione (%2). Un Kuro precedente non può aprire gli elenchi di uno più recente, quindi non è stato modificato nulla.
nb.Updating=Kuro %1 er installert. Det oppdateres til %2.
nb.UpdateButton=&Oppdater
nb.SameVersion=Kuro %1 er allerede installert. Vil du installere det på nytt?
nb.NewerInstalled=Kuro %1 er installert og er nyere enn dette installasjonsprogrammet (%2). En eldre Kuro kan ikke åpne listene til en nyere, så ingenting er endret.
ko.Updating=Kuro %1이(가) 설치되어 있습니다. %2(으)로 업데이트합니다.
ko.UpdateButton=업데이트(&U)
ko.SameVersion=Kuro %1이(가) 이미 설치되어 있습니다. 다시 설치하시겠습니까?
ko.NewerInstalled=설치된 Kuro %1이(가) 이 설치 프로그램(%2)보다 최신입니다. 이전 버전의 Kuro는 최신 버전의 목록을 열 수 없으므로 아무것도 변경하지 않았습니다.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#Files}\kuro.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Files}\*LICENSE*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The id matches the one Kuro uses for its notifications.
Name: "{autoprograms}\Kuro"; Filename: "{app}\kuro.exe"; AppUserModelID: "Kuro.Tracker"
Name: "{autodesktop}\Kuro"; Filename: "{app}\kuro.exe"; AppUserModelID: "Kuro.Tracker"; Tasks: desktopicon

[Registry]
; Written by Kuro when it starts.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\Kuro.Tracker"; Flags: dontcreatekey uninsdeletekey

[Run]
Filename: "{app}\kuro.exe"; Description: "{cm:LaunchProgram,Kuro}"; Flags: nowait postinstall skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  LinkKey = 'Software\Classes\kuro';
  AppKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{430942B2-A6DE-46B7-96A7-DDF405974F13}_is1';

var
  { Version being replaced by a newer one, or empty. }
  UpdatingFrom: String;

{ Below zero when the installed version is older than this one; false when there is none. }
function CompareInstalled(var Installed: String; var Order: Integer): Boolean;
var
  Dir: String;
  Old, New: Int64;
begin
  Result := RegQueryStringValue(HKCU, AppKey, 'DisplayVersion', Installed) and
    RegQueryStringValue(HKCU, AppKey, 'InstallLocation', Dir) and
    { The folder may have been deleted by hand. }
    FileExists(AddBackslash(Dir) + 'kuro.exe') and
    StrToVersion(Installed, Old) and StrToVersion('{#AppVersion}', New);
  if Result then
    Order := ComparePackedVersion(Old, New);
end;

function InitializeSetup: Boolean;
var
  Installed: String;
  Order: Integer;
begin
  Result := True;
  if not CompareInstalled(Installed, Order) then
    Exit;
  if Order < 0 then
    UpdatingFrom := Installed
  else if Order = 0 then
    { Without anyone to ask, installing again repairs. }
    Result := SuppressibleMsgBox(FmtMessage(CustomMessage('SameVersion'), [Installed]),
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDYES) = IDYES
  else
  begin
    SuppressibleMsgBox(FmtMessage(CustomMessage('NewerInstalled'), [Installed, '{#AppVersion}']),
      mbError, MB_OK, IDOK);
    Result := False;
  end;
end;

{ An update keeps the choices made the first time. }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = wpSelectTasks) and (UpdatingFrom <> '');
end;

procedure CurPageChanged(PageID: Integer);
begin
  if (PageID = wpReady) and (UpdatingFrom <> '') then
  begin
    WizardForm.ReadyLabel.Caption :=
      FmtMessage(CustomMessage('Updating'), [UpdatingFrom, '{#AppVersion}']);
    WizardForm.NextButton.Caption := CustomMessage('UpdateButton');
  end;
end;

function Exe: String;
begin
  Result := ExpandConstant('{app}\kuro.exe');
end;

procedure QuitKuro;
var
  Code: Integer;
begin
  if FileExists(Exe) then
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitKuro;
  Result := '';
end;

{ Another copy of Kuro may own the entry, so look before removing it. }
function PointsHere(Key, Name: String): Boolean;
var
  Command: String;
begin
  Result := RegQueryStringValue(HKCU, Key, Name, Command) and
    (Pos(Lowercase(Exe), Lowercase(Command)) > 0);
end;

procedure CurUninstallStepChanged(Step: TUninstallStep);
begin
  if Step <> usUninstall then
    Exit;
  QuitKuro;
  if PointsHere(RunKey, 'Kuro') then
    RegDeleteValue(HKCU, RunKey, 'Kuro');
  if PointsHere(LinkKey + '\shell\open\command', '') then
    RegDeleteKeyIncludingSubkeys(HKCU, LinkKey);
end;
