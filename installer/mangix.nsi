; Comix installer.
;
; Per-user install: no administrator prompt, everything under the user's own
; profile and HKCU. That suits a personal tool and keeps SmartScreen quieter
; than a machine-wide install would.
;
; Build it with the two shipped files sitting beside this script:
;
;   target\release\comix.exe  ->  installer\comix.exe
;   pdfium.dll                ->  installer\pdfium.dll
;   makensis comix.nsi
;
; If you are not shipping PDF support yet, build with:
;
;   makensis /DNO_PDFIUM comix.nsi

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!define APP      "Comix"
!define VERSION  "0.1.0"
!define PUBLISHER "billy"
!define EXE      "comix.exe"
!define UNINST   "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"

Name "${APP}"
OutFile "Comix-${VERSION}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "Software\${APP}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} comic reader"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" ""

!define MUI_ICON "..\\assets\\comix.ico"
!define MUI_UNICON "..\\assets\\comix.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Open ${APP}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Registers one extension against the Comix file type. Writing to HKCU means
; no elevation, and it only affects this user.
!macro AssociateExt EXT
  WriteRegStr HKCU "Software\Classes\${EXT}" "" "Comix.Comic"
  WriteRegStr HKCU "Software\Classes\${EXT}\OpenWithProgids" "Comix.Comic" ""
!macroend

!macro UnassociateExt EXT
  ; Only surrender the extension if it is still pointing at us: another
  ; reader may have claimed it since install, and stomping that would be rude.
  ReadRegStr $0 HKCU "Software\Classes\${EXT}" ""
  StrCmp $0 "Comix.Comic" 0 +2
    DeleteRegKey HKCU "Software\Classes\${EXT}"
  DeleteRegValue HKCU "Software\Classes\${EXT}\OpenWithProgids" "Comix.Comic"
!macroend

Section "Install"
  SetOutPath "$INSTDIR"
  File "${EXE}"
!ifndef NO_PDFIUM
  ; PDF rendering. Comix looks for this next to the executable and simply
  ; declines to open PDFs when it is absent.
  File "pdfium.dll"
!endif

  ; File type, used by every extension below.
  WriteRegStr HKCU "Software\Classes\Comix.Comic" "" "Comic book"
  WriteRegStr HKCU "Software\Classes\Comix.Comic\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "Software\Classes\Comix.Comic\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'

  !insertmacro AssociateExt ".cbz"
  !insertmacro AssociateExt ".cbr"
  !insertmacro AssociateExt ".cb7"
!ifndef NO_PDFIUM
  ; Deliberately registered only as an alternative under "Open with", not as
  ; the default: taking over every PDF on the machine would not be welcome.
  WriteRegStr HKCU "Software\Classes\.pdf\OpenWithProgids" "Comix.Comic" ""
!endif

  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKCU "Software\${APP}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Report the installed size in Apps & features.
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST}" "EstimatedSize" "$0"

  ; Tell the shell the associations changed, so icons refresh immediately.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\pdfium.dll"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP}.lnk"

  !insertmacro UnassociateExt ".cbz"
  !insertmacro UnassociateExt ".cbr"
  !insertmacro UnassociateExt ".cb7"
  DeleteRegValue HKCU "Software\Classes\.pdf\OpenWithProgids" "Comix.Comic"
  DeleteRegKey HKCU "Software\Classes\Comix.Comic"

  DeleteRegKey HKCU "${UNINST}"
  DeleteRegKey HKCU "Software\${APP}"

  ; Reading positions and settings.
  RMDir /r "$APPDATA\comix"

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
