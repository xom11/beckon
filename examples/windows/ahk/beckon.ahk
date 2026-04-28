; beckon bindings for Windows via AutoHotkey v2.
;
; Save as beckon.ahk anywhere and either:
;   - double-click to run for the current session, or
;   - drop a shortcut in shell:startup so it loads at login.
;
; Requires:
;   - AutoHotkey v2 (https://www.autohotkey.com/)
;   - beckon installed. `cargo install --git https://github.com/xom11/beckon`
;     drops it at %USERPROFILE%\.cargo\bin\beckon.exe — the BeckonExe path
;     below assumes that. Adjust if you installed it elsewhere.
;
; Modifier combo: Ctrl+Win+Alt (`^#!`). Pick whatever you like — these
; rarely conflict with built-in Windows shortcuts.
;
; Discover ids on your machine:
;   beckon -L            list installed apps (Start Menu shortcuts)
;   beckon -l            list currently running apps
;   beckon -s claude     fuzzy search
;   beckon -r Claude     validate an id

#Requires AutoHotkey v2.0

BeckonExe := A_UserProfile . "\.cargo\bin\beckon.exe"

Beckon(name) {
    try Run('"' BeckonExe '" "' name '"', , "Hide")
}

^#!Space:: Beckon("Windows Terminal")
^#!c::     Beckon("Claude")
^#!b::     Beckon("Brave")
^#!e::     Beckon("Cursor")
^#!d::     Beckon("Discord")
