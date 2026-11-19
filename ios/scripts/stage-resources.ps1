# Stage the official MJCF, STL meshes and ONNX policies into the iOS bundle.
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$src  = Join-Path $root 'upstream\microduck_rl\src\mjlab_microduck\robot\microduck'
# DuckResources, not Resources: the Xcode project copies DuckResources into
# the bundle and a root-level Resources folder makes codesign treat the app
# as a deep bundle ("Info.plist=not bound").
$dst  = Join-Path $root 'ios\DuckApp\DuckResources'

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
# The official pet-detect model: the shell posts 16 kHz mono PCM to the core
# only when this file is staged. The core resolves `<root>/models/
# pet_detect.onnx` beside the policy set; the ticket's `pet-detect/` path is
# staged too. A missing model is a supported state (no stream, no network).
$petDetect = Join-Path $root 'upstream\microduck\pet-detect\models\pet_detect.onnx'
if (Test-Path $petDetect) {
  New-Item -ItemType Directory -Force (Join-Path $dst 'pet-detect') | Out-Null
  Copy-Item $petDetect (Join-Path $dst 'pet-detect\pet_detect.onnx') -Force
  Copy-Item $petDetect (Join-Path $dst 'models\pet_detect.onnx') -Force
}
# `robotd/emotions/`: the real duck's thirteen scripted moods and voices.
$emotions = Join-Path $root 'upstream\microduck\robotd\emotions'
if (Test-Path $emotions) {
  New-Item -ItemType Directory -Force (Join-Path $dst 'emotions') | Out-Null
  Copy-Item (Join-Path $emotions '*.json'), (Join-Path $emotions '*.wav') (Join-Path $dst 'emotions') -Force
}

# The shader lives outside $dst so the wipe above cannot delete it; the
# renderer needs it in the bundle or the 3D view stays black.
$shader = Join-Path $root 'ios\DuckApp\Shaders\Shaders.metal'
if (-not (Test-Path $shader)) { throw "missing $shader - the app would ship without a Metal library" }
Copy-Item $shader (Join-Path $dst 'Shaders.metal') -Force

$size = (Get-ChildItem $dst -Recurse -File | Measure-Object Length -Sum).Sum
"staged resources into $dst ($([math]::Round($size/1MB,1)) MB)"
