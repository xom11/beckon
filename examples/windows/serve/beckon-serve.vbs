' Launch `beckon --serve` with no console window, still logging stderr.
'
' beckon.exe is a console application, so a Scheduled Task that runs it
' — directly or through cmd.exe — leaves a visible console window on the
' desktop for as long as the daemon lives. (Measured on Windows 11 build
' 26200: a task-launched console process reports IsWindowVisible = true
' for its own GetConsoleWindow.)
'
' This shim starts the same cmd.exe redirect hidden (0) and without
' waiting (False), so you keep the log and lose the window. It is the
' same thing the AutoHotkey example does with Run(..., "Hide").
'
' Edit the three paths below, then point the Scheduled Task at:
'     wscript.exe "C:\path\to\beckon-serve.vbs"
'
' Note: VBScript is a deprecated Windows feature-on-demand. It is still
' present by default today (verified on build 26200), but if it is ever
' removed from your install, fall back to the plain cmd.exe action in
' beckon-serve.xml and accept the window.

beckonExe = "C:\Users\YOUR_USERNAME\.cargo\bin\beckon.exe"
config    = "C:\Users\YOUR_USERNAME\.config\beckon\apps.toml"
logFile   = "C:\Users\YOUR_USERNAME\AppData\Local\beckon\serve.log"

cmd = "cmd /c """"" & beckonExe & """ --serve """ & config & """ 2> """ & logFile & """"""

CreateObject("WScript.Shell").Run cmd, 0, False
