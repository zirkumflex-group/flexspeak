@echo off
rem Runs once at the end of the unattended install. Only enables SSH; everything
rem else is installed over SSH (setup.ps1) so it can be watched and rerun.
msiexec /i C:\OEM\OpenSSH-Win64.msi /qn /norestart
mkdir C:\ProgramData\ssh 2>nul
copy /y C:\OEM\authorized_keys C:\ProgramData\ssh\administrators_authorized_keys
icacls C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F"
reg add HKLM\SOFTWARE\OpenSSH /v DefaultShell /t REG_SZ /d C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe /f
netsh advfirewall firewall add rule name="OpenSSH" dir=in action=allow protocol=TCP localport=22
sc config sshd start= auto
net start sshd
