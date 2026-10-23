param([string]$Python = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'hash-file.ps1')
$venvPython = Join-Path $PSScriptRoot '.venv-build/Scripts/python.exe'
$validVenv = $false
if (Test-Path $venvPython) {
  & $venvPython -c 'import sys; sys.exit(0 if sys.version_info[:2] == (3, 12) else 1)'
  $validVenv = $LASTEXITCODE -eq 0
}
if (-not $validVenv) {
  if (-not $Python -and (Get-Command py -ErrorAction SilentlyContinue)) {
    $ErrorActionPreference = 'Continue'
    try {
      $candidate = & py -3.12 -c 'import sys; print(sys.executable)' 2>$null
      if ($LASTEXITCODE -eq 0) { $Python = $candidate }
    } finally { $ErrorActionPreference = 'Stop' }
  }
  if (-not $Python) {
    # uv-managed Python is not necessarily registered with the Windows launcher.
    $uvRoot = Join-Path $env:APPDATA 'uv/python'
    $candidate = Get-ChildItem -Path (Join-Path $uvRoot 'cpython-3.12*/python.exe') -ErrorAction SilentlyContinue | Select-Object -First 1
    $Python = if ($candidate) { $candidate.FullName } else { 'python' }
  }
  & $Python -c 'import sys; sys.exit(0 if sys.version_info[:2] == (3, 12) else 1)'
  if ($LASTEXITCODE -ne 0) { throw 'OCR build requires Python 3.12. Run build-worker.ps1 -Python <path-to-python.exe>.' }
  & $Python -m venv (Join-Path $PSScriptRoot '.venv-build')
  if ($LASTEXITCODE -ne 0) { throw 'Cannot create OCR build environment' }
}
# Recover an interrupted venv creation before installing the locked dependencies.
& $venvPython -m ensurepip --upgrade
if ($LASTEXITCODE -ne 0) { throw 'Cannot bootstrap OCR build pip' }
& $venvPython -m pip install -r (Join-Path $PSScriptRoot 'requirements.lock.txt')
if ($LASTEXITCODE -ne 0) { throw 'Cannot install OCR dependencies' }
Push-Location $PSScriptRoot
try {
  Copy-Item 'worker.spec.template' 'worker.spec' -Force
  & $venvPython -m PyInstaller --noconfirm --clean worker.spec
  if ($LASTEXITCODE -ne 0) { throw 'Cannot package OCR worker' }
  $fingerprint = @{}
  foreach ($name in @('ocr_worker.py', 'requirements.lock.txt', 'worker.spec.template')) {
    $fingerprint[$name] = Get-OcrFileHash (Join-Path $PSScriptRoot $name)
  }
  $fingerprint | ConvertTo-Json | Set-Content 'dist/livecaption-ocr-worker/worker-build.json'
} finally { Pop-Location }
