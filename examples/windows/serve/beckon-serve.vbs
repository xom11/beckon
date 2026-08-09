' Launch `beckon --serve` with no console window.
'
' beckon.exe is a console application, so a Scheduled Task that runs it
' directly leaves an empty console window on screen for as long as the
' daemon lives. This three-line shim starts it hidden (0) without
' waiting (False) — the same thing the AutoHotkey example does with
' Run(..., "Hide").
'
' Edit both paths below to match your machine, then point the Scheduled
' Task at:  wscript.exe "C:\path\to\beckon-serve.vbs"
'
' Note: VBScript is a deprecated Windows feature-on-demand. It is still
' present by default today, but if it is removed from your install, use
' the direct-exec action in beckon-serve.xml instead and accept the
' visible console window. See the README for the trade-off.

beckonExe = "C:\Users\YOUR_USERNAME\.cargo\bin\beckon.exe"
config    = "C:\Users\YOUR_USERNAME\.config\beckon\apps.toml"

CreateObject("WScript.Shell").Run """" & beckonExe & """ --serve """ & config & """", 0, False
