param(
    [Parameter(Mandatory=$true)][string]$Python,
    [string]$CudaBin = '',
    [string]$CudnnBin = ''
)
$ErrorActionPreference = 'Stop'
$WorkerDir = $PSScriptRoot
$VenvDir = Join-Path $WorkerDir '.venv-build'
$BuildDir = Join-Path $WorkerDir 'build'
$DistDir = Join-Path $WorkerDir 'dist'
& $Python -m venv $VenvDir
$VenvPython = Join-Path $VenvDir 'Scripts\python.exe'
& $VenvPython -m pip install --requirement (Join-Path $WorkerDir 'requirements.lock.txt')
& $VenvPython -m PyInstaller --noconfirm --clean --onedir --console --name livecaption-asr-worker `
  --workpath $BuildDir --distpath $DistDir --specpath $WorkerDir `
  --collect-all faster_whisper --collect-all ctranslate2 --collect-all soundcard `
  (Join-Path $WorkerDir 'asr_worker.py')
$InternalDir = Join-Path $WorkerDir 'dist\livecaption-asr-worker\_internal'
$CudaFiles = @('cublas64_12.dll', 'cublasLt64_12.dll')
if (-not $CudaBin) {
  $CudaRoot = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA'
  if (Test-Path -LiteralPath $CudaRoot) {
    $CudaVersions = Get-ChildItem -LiteralPath $CudaRoot -Directory | Sort-Object Name -Descending
    foreach ($Version in $CudaVersions) {
      $Candidate = Join-Path $Version.FullName 'bin'
      if ((Test-Path -LiteralPath (Join-Path $Candidate 'cublas64_12.dll')) -and
          (Test-Path -LiteralPath (Join-Path $Candidate 'cublasLt64_12.dll'))) {
        $CudaBin = $Candidate
        break
      }
    }
  }
}
if (-not $CudaBin) {
  throw 'CUDA 12 runtime not found. Pass -CudaBin pointing to a directory containing cublas64_12.dll and cublasLt64_12.dll.'
}
foreach ($File in $CudaFiles) {
  $Source = Join-Path $CudaBin $File
  if (-not (Test-Path -LiteralPath $Source)) { throw "Missing CUDA runtime: $Source" }
  Copy-Item -LiteralPath $Source -Destination $InternalDir -Force
}
if (-not $CudnnBin) {
  $Candidates = @(
    (Join-Path $VenvDir 'Lib\site-packages\nvidia\cudnn\bin'),
    'C:\Program Files\NVIDIA\CUDNN\v9.0\bin\12.4',
    $CudaBin
  )
  $CudnnBin = $Candidates | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'cudnn64_9.dll') } | Select-Object -First 1
}
if (-not $CudnnBin -or -not (Test-Path -LiteralPath (Join-Path $CudnnBin 'cudnn64_9.dll'))) {
  throw 'cuDNN 9 runtime not found. Pass -CudnnBin pointing to the directory containing cudnn64_9.dll.'
}
Get-ChildItem -LiteralPath $CudnnBin -Filter 'cudnn*64_9.dll' | Copy-Item -Destination $InternalDir -Force
Write-Host "Worker built at $WorkerDir\dist\livecaption-asr-worker\livecaption-asr-worker.exe"
