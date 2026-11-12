# Provisions the flexspeak CI VM: build tools, then the GitHub runners. Safe to rerun;
# downloads are kept in C:\ci\downloads. Runs over SSH as the VM's admin user `ci`:
#   /srv/flexspeak-ci/vm powershell -File C:\OEM\setup.ps1 -Token <registration token> -Password <ci's password>
param([string]$Token, [string]$Password, [int]$Runners = 2)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = 'Tls12'
$downloads = 'C:\ci\downloads'
New-Item -ItemType Directory -Force $downloads | Out-Null

function Download($url, $name = (Split-Path $url -Leaf)) {
    $file = Join-Path $downloads $name
    if (-not (Test-Path $file)) {
        Write-Host "Downloading $name"
        Invoke-WebRequest $url -OutFile "$file.part" -UseBasicParsing
        Move-Item "$file.part" $file
    }
    $file
}
function Install($file, [string[]]$arguments) {
    Write-Host "Installing $(Split-Path $file -Leaf)"
    $process = Start-Process $file -ArgumentList $arguments -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -notin 0, 3010) { throw "$file exited with $($process.ExitCode)" }
}
function Add-MachinePath($dir) {
    $path = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    if (($path -split ';') -notcontains $dir) { [Environment]::SetEnvironmentVariable('Path', "$path;$dir", 'Machine') }
}

# Long paths (Cargo's registry), and updates that never restart the VM in the middle of a build.
Set-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem LongPathsEnabled 1
$au = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
New-Item -Force $au | Out-Null
Set-ItemProperty $au AUOptions 3        # download automatically, install when told
Set-ItemProperty $au NoAutoRebootWithLoggedOnUsers 1

# What GitHub's windows-2022 image has and the workflows use: MSVC + Windows SDK, Git with
# bash, Python, PowerShell 7 (the default `run` shell), CMake (bundled Opus) and rustup.
if (-not (Test-Path 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC')) {
    Install (Download 'https://aka.ms/vs/17/release/vs_buildtools.exe' 'vs_buildtools.exe') @(
        '--quiet', '--wait', '--norestart', '--nocache',
        '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended')
}
if (-not (Test-Path 'C:\Program Files\Git\bin\bash.exe')) {
    Install (Download 'https://github.com/git-for-windows/git/releases/download/v2.56.1.windows.1/Git-2.56.1-64-bit.exe') @(
        '/VERYSILENT', '/NORESTART', '/NOCANCEL', '/SP-')
}
# bash for `shell: bash`. Not Git's usr\bin: its link.exe would shadow MSVC's linker.
Add-MachinePath 'C:\Program Files\Git\bin'
if (-not (Test-Path 'C:\Program Files\Python314\python.exe')) {
    Install (Download 'https://www.python.org/ftp/python/3.14.10/python-3.14.10-amd64.exe') @(
        '/quiet', 'InstallAllUsers=1', 'PrependPath=1', 'Include_test=0')
}
if (-not (Test-Path 'C:\Program Files\PowerShell\7\pwsh.exe')) {
    $zip = Download 'https://github.com/PowerShell/PowerShell/releases/download/v7.7.0/PowerShell-7.7.0-win-x64.zip'
    Expand-Archive $zip 'C:\Program Files\PowerShell\7' -Force
}
Add-MachinePath 'C:\Program Files\PowerShell\7'
if (-not (Test-Path 'C:\Program Files\CMake\bin\cmake.exe')) {
    $msi = Download 'https://github.com/Kitware/CMake/releases/download/v4.4.5/cmake-4.4.5-windows-x86_64.msi'
    Install 'msiexec.exe' @('/i', $msi, '/qn', 'ADD_CMAKE_TO_PATH=System')
}
[Environment]::SetEnvironmentVariable('RUSTUP_HOME', 'C:\rust\rustup', 'Machine')
[Environment]::SetEnvironmentVariable('CARGO_HOME', 'C:\rust\cargo', 'Machine')
$env:RUSTUP_HOME = 'C:\rust\rustup'; $env:CARGO_HOME = 'C:\rust\cargo'
if (-not (Test-Path 'C:\rust\cargo\bin\rustup.exe')) {
    Install (Download 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe') @(
        '-y', '--no-modify-path', '--profile', 'minimal', '--default-toolchain', 'none')
}
Add-MachinePath 'C:\rust\cargo\bin'
& 'C:\Program Files\Git\cmd\git.exe' config --system core.longpaths true

# Build output kept between jobs (the point of this runner), outside the checkouts, which
# actions/checkout cleans. The job hook lives here too.
New-Item -ItemType Directory -Force 'C:\ci\target', 'C:\ci\opus' | Out-Null
Copy-Item -Force C:\OEM\allow-job.ps1 C:\ci\allow-job.ps1

if (-not $Token) { Write-Host 'Tools installed; no -Token, so the runners were left alone.'; exit 0 }
$runnerZip = Download 'https://github.com/actions/runner/releases/download/v2.338.2/actions-runner-win-x64-2.338.2.zip'
foreach ($n in 1..$Runners) {
    $dir = "C:\actions-runner\$n"
    if (Test-Path "$dir\.runner") { Write-Host "Runner $n is already configured"; continue }
    New-Item -ItemType Directory -Force $dir | Out-Null
    Expand-Archive $runnerZip $dir -Force
    # Read by the runner at start: the hook that refuses untrusted jobs, and where the
    # workflows keep build output.
    Set-Content "$dir\.env" -Encoding ascii -Value @(
        'ACTIONS_RUNNER_HOOK_JOB_STARTED=C:\ci\allow-job.ps1',
        'FLEXSPEAK_CI_DIR=C:\ci')
    Push-Location $dir
    & .\config.cmd --unattended --url https://github.com/zirkumflex-group/flexspeak --token $Token `
        --name "beelink-$n" --labels flexspeak-win --work "C:\w$n" --replace `
        --runasservice --windowslogonaccount '.\ci' --windowslogonpassword $Password
    if ($LASTEXITCODE) { throw "Runner $n failed to configure" }
    Pop-Location
}
