!macro customInstall
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="REFEREE Upscaler" dir=in action=allow protocol=TCP localport=14002'
!macroend

!macro customUnInstall
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="REFEREE Upscaler"'
!macroend
