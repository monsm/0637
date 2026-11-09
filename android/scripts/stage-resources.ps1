# Stage the official MJCF, STL meshes and ONNX policies into the Android assets.
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$src  = Join-Path $root 'upstream\microduck_rl\src\mjlab_microduck\robot\microduck'
$dst  = Join-Path $root 'android\app\src\main\assets'

if (-not (Test-Path $src))  { throw "missing $src - clone microduck_rl into upstream/ first" }
if (-not (Test-Path (Join-Path $root 'policies'))) { throw "missing policies/ - download the official ONNX set first" }

if (Test-Path $dst) { Remove-Item $dst -Recurse -Force -Confirm:$false }
New-Item -ItemType Directory -Force (Join-Path $dst 'robot\microduck\assets'), (Join-Path $dst 'policies'), (Join-Path $dst 'models') | Out-Null

Copy-Item (Join-Path $src '*.xml') (Join-Path $dst 'robot\microduck') -Force
Copy-Item (Join-Path $src 'assets\*.stl') (Join-Path $dst 'robot\microduck\assets') -Force
Copy-Item (Join-Path $root 'policies\*.onnx') (Join-Path $dst 'policies') -Force
$manifest = Join-Path $root 'policies\manifest.json'
if (Test-Path $manifest) { Copy-Item $manifest (Join-Path $dst 'policies') -Force }
$pickup = Join-Path $root 'upstream\microduck\duck-control\models\pickup_detector.onnx'
if (Test-Path $pickup) { Copy-Item $pickup (Join-Path $dst 'models\pickup_detector.onnx') -Force }

$size = (Get-ChildItem $dst -Recurse -File | Measure-Object Length -Sum).Sum
"staged Android assets ($([math]::Round($size/1MB,1)) MB)"
