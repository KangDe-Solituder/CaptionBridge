param([string]$Python = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../build-support/worker-common.ps1')
$venvPython = Initialize-WorkerVenv $PSScriptRoot $Python @(12)
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
  Write-WorkerFingerprint $PSScriptRoot (Join-Path $PSScriptRoot 'dist/livecaption-ocr-worker/worker-build.json') (Get-WorkerInputs 'ocr')
} finally { Pop-Location }

& (Join-Path $PSScriptRoot 'verify-worker.ps1')
