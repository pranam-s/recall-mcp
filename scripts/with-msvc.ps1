# Run a command with the MSVC (Visual Studio Build Tools) environment loaded.
#
# Usage:
#   .\scripts\with-msvc.ps1 cargo test --workspace --locked
#   .\scripts\with-msvc.ps1 cargo llvm-cov -p recall-mcp-core ...
#
# Locates vswhere.exe, asks it for the newest VS 2022 Build Tools install,
# invokes vcvars64.bat in a child cmd.exe, imports the resulting environment
# into this PowerShell process, then forwards all arguments to the command.
# Exit code is the forwarded command's exit code.
# All arguments are forwarded verbatim, including flags such as `-p` or
# `--workspace` (a `param` block would try to bind those as PowerShell
# parameters).
$Command = @($args)

$ErrorActionPreference = 'Stop'

if (-not $Command -or $Command.Count -eq 0) {
    Write-Error 'usage: scripts/with-msvc.ps1 <command> [args...]'
    exit 2
}

$vswhereCandidates = @(@(
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'),
    (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\Installer\vswhere.exe')
) | Where-Object { $_ -and (Test-Path $_) })

if (-not $vswhereCandidates) {
    Write-Error 'vswhere.exe not found; install Visual Studio Build Tools with the C++ workload'
    exit 2
}
$vswhere = $vswhereCandidates[0]

$installPath = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
if (-not $installPath) {
    # Some Build Tools installs are present on disk but absent from the
    # vswhere catalog (seen in local testing). Fall back to the well-known
    # locations, newest first.
    $installPath = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools'),
        (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\2022\BuildTools'),
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\Enterprise'),
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\Professional'),
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\Community')
    ) | Where-Object { $_ -and (Test-Path (Join-Path $_ 'VC\Auxiliary\Build\vcvars64.bat')) } |
        Select-Object -First 1
}
if (-not $installPath) {
    Write-Error 'no Visual Studio installation with the C++ build tools was found'
    exit 2
}

$vcvars = Join-Path $installPath 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path $vcvars)) {
    Write-Error "vcvars64.bat not found under $installPath"
    exit 2
}

# Capture the environment variables vcvars64.bat sets without polluting the
# caller's session: run it in a child cmd.exe, then import the diff.
$before = @{}
Get-ChildItem Env: | ForEach-Object { $before[$_.Name] = $_.Value }

$captured = & cmd.exe /c "`"$vcvars`" >nul 2>&1 && set" 2>$null
foreach ($line in $captured) {
    $idx = $line.IndexOf('=')
    if ($idx -gt 0) {
        $name = $line.Substring(0, $idx)
        $value = $line.Substring($idx + 1)
        Set-Item -Path "Env:$name" -Value $value
    }
}

if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    Write-Error 'cl.exe still not on PATH after loading vcvars64.bat'
    exit 2
}

$exe = $Command[0]
$forward = @()
if ($Command.Count -gt 1) {
    $forward = $Command[1..($Command.Count - 1)]
}
# Native tools write progress to stderr; with ErrorActionPreference=Stop,
# Windows PowerShell turns that into a terminating NativeCommandError. The
# forwarded command's exit code is the source of truth.
$ErrorActionPreference = 'Continue'
& $exe @forward
exit $LASTEXITCODE
