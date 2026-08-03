param(
    [Parameter(Mandatory=$true)][string]$Python,
    [string]$CudaBin = '',
    [string]$CudnnBin = '',
    [switch]$BundleGpuRuntime
)
$ErrorActionPreference = 'Stop'
$WorkerDir = $PSScriptRoot
$VenvDir = Join-Path $WorkerDir '.venv-build'
$BuildDir = Join-Path $WorkerDir 'build'
$DistDir = Join-Path $WorkerDir 'dist'
$VenvPython = Join-Path $VenvDir 'Scripts\python.exe'
if (-not (Test-Path -LiteralPath $VenvPython)) {
  & $Python -m venv $VenvDir
  if ($LASTEXITCODE -ne 0) { throw "Failed to create worker virtual environment (exit $LASTEXITCODE)." }
} else {
  Write-Host "Reusing worker virtual environment at $VenvDir"
}
& $VenvPython -m pip install --requirement (Join-Path $WorkerDir 'requirements.lock.txt')
if ($LASTEXITCODE -ne 0) { throw "Failed to install worker requirements (exit $LASTEXITCODE)." }
if ($BundleGpuRuntime -and -not $CudnnBin) {
  & $VenvPython -m pip install --requirement (Join-Path $WorkerDir 'gpu-runtime-requirements.lock.txt')
  if ($LASTEXITCODE -ne 0) { throw "Failed to install pinned cuDNN build runtime (exit $LASTEXITCODE)." }
}
& $VenvPython -m PyInstaller --noconfirm --clean --onedir --console --name livecaption-asr-worker `
  --workpath $BuildDir --distpath $DistDir --specpath $WorkerDir `
  --hidden-import pkg_resources `
  --collect-all faster_whisper --collect-all ctranslate2 --collect-all soundcard `
  (Join-Path $WorkerDir 'asr_worker.py')
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed (exit $LASTEXITCODE)." }
$InternalDir = Join-Path $WorkerDir 'dist\livecaption-asr-worker\_internal'
$CudaFiles = @('cublas64_12.dll', 'cublasLt64_12.dll')
$CudnnFiles = @(
  'cudnn64_9.dll',
  'cudnn_adv64_9.dll',
  'cudnn_cnn64_9.dll',
  'cudnn_engines_precompiled64_9.dll',
  'cudnn_engines_runtime_compiled64_9.dll',
  'cudnn_graph64_9.dll',
  'cudnn_heuristic64_9.dll',
  'cudnn_ops64_9.dll'
)
$GpuRuntimeFiles = $CudaFiles + $CudnnFiles
# PyInstaller can collect the small ctranslate2 entry DLL even when the
# optional GPU runtime is not requested. Remove all native GPU files first so
# the default installer stays lightweight and always uses the cached/system
# runtime selected by the Rust launcher.
Get-ChildItem -LiteralPath $InternalDir -Recurse -File -ErrorAction SilentlyContinue |
  Where-Object { $GpuRuntimeFiles -contains $_.Name } |
  Remove-Item -Force
if ($BundleGpuRuntime) {
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
    $CudnnBin = $Candidates | Where-Object {
      $Candidate = $_
      -not ($CudnnFiles | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Candidate $_)) })
    } | Select-Object -First 1
  }
  if (-not $CudnnBin) {
    throw 'Complete cuDNN 9 runtime not found. Pass -CudnnBin pointing to the directory containing the full cuDNN 9 DLL set.'
  }
  $MissingCudnnFiles = $CudnnFiles | Where-Object {
    -not (Test-Path -LiteralPath (Join-Path $CudnnBin $_))
  }
  if ($MissingCudnnFiles) {
    throw "Incomplete cuDNN 9 runtime at $CudnnBin. Missing: $($MissingCudnnFiles -join ', ')"
  }
  foreach ($File in $CudnnFiles) {
    Copy-Item -LiteralPath (Join-Path $CudnnBin $File) -Destination $InternalDir -Force
  }
  $MissingBundledCudnnFiles = $CudnnFiles | Where-Object {
    -not (Test-Path -LiteralPath (Join-Path $InternalDir $_))
  }
  if ($MissingBundledCudnnFiles) {
    throw "Worker bundle is missing cuDNN files: $($MissingBundledCudnnFiles -join ', ')"
  }
  Write-Host 'Bundled CUDA 12 and cuDNN 9 runtime libraries.'
} else {
  Write-Host 'GPU runtime libraries excluded. The installed app will use the downloaded or system CUDA 12/cuDNN 9 runtime.'
  # Tauri copies resources into target/release and target/debug without
  # clearing files removed from the source directory. Remove stale native
  # runtime files from previous -BundleGpuRuntime builds so a later package
  # cannot accidentally include the old 1+ GB payload.
  $TargetRoot = Join-Path (Split-Path $WorkerDir -Parent) 'target'
  foreach ($Profile in @('debug', 'release')) {
    $TargetWorkerDir = Join-Path $TargetRoot "$Profile\asr-worker"
    if (Test-Path -LiteralPath $TargetWorkerDir) {
      Get-ChildItem -LiteralPath $TargetWorkerDir -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $GpuRuntimeFiles -contains $_.Name } |
        Remove-Item -Force
    }
  }
}
$WorkerExe = Join-Path $WorkerDir 'dist\livecaption-asr-worker\livecaption-asr-worker.exe'
$ProbeOutput = @(
  '{"command":"probe_dependencies"}',
  '{"command":"shutdown"}'
) | & $WorkerExe
if ($LASTEXITCODE -ne 0 -or -not ($ProbeOutput -match '"type": "dependency_probe"') -or
    ($BundleGpuRuntime -and (-not ($ProbeOutput -match '"cuda_runtime_loaded": true') -or
      -not ($ProbeOutput -match '"cudnn_runtime_loaded": true')))) {
  throw 'Worker build completed, but its dependency-probe protocol check failed.'
}
Write-Host "Worker built and protocol-checked at $WorkerExe"
