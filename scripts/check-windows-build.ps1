$ErrorActionPreference = "Stop"

function Test-Tool {
  param([string]$Name)

  $tool = Get-Command $Name -ErrorAction SilentlyContinue
  if ($tool) {
    Write-Host "[ok] $Name -> $($tool.Source)"
    return $true
  }

  Write-Host "[missing] $Name"
  return $false
}

Write-Host "Checking Windows desktop build prerequisites..."
$hasCargo = Test-Tool "cargo.exe"
$hasRustc = Test-Tool "rustc.exe"
$hasNode = Test-Tool "node.exe"
$hasNpm = (Test-Tool "npm.cmd") -or (Test-Tool "npm.exe")

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vcvars = $null
if (Test-Path $vswhere) {
  $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if ($installPath) {
    $candidate = Join-Path $installPath "VC\Auxiliary\Build\vcvars64.bat"
    if (Test-Path $candidate) {
      $vcvars = $candidate
      Write-Host "[ok] vcvars64.bat -> $vcvars"
    }
  }
}

$hasLink = (Test-Tool "link.exe") -or [bool]$vcvars
$hasCl = (Test-Tool "cl.exe") -or [bool]$vcvars
$hasRc = (Test-Tool "rc.exe") -or (Test-Tool "llvm-rc.exe") -or [bool](Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter rc.exe -ErrorAction SilentlyContinue | Select-Object -First 1)

Write-Host ""
if ($hasCargo -and $hasRustc -and $hasNode -and $hasNpm -and $hasLink -and $hasCl -and $hasRc) {
  Write-Host "Ready: this machine can build the desktop exe."
  exit 0
}

Write-Host "Not ready yet."
Write-Host "Install Visual Studio Build Tools with 'Desktop development with C++' and a Windows SDK."
Write-Host "After installing, open a new terminal and run: npm run build:exe"
exit 1
