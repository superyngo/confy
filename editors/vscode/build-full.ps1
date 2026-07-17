# Complete build script for confy VS Code extension
# Run this from editors/vscode directory

Write-Host "=== Building confy VS Code Extension ===" -ForegroundColor Cyan

# Step 1: Build web bundle
Write-Host "`n[1/4] Building web bundle..." -ForegroundColor Yellow
Set-Location ..\..\web
node build.mjs
if ($LASTEXITCODE -ne 0) { throw "Web build failed" }

# Step 2: Assemble web/dist
Write-Host "`n[2/4] Assembling web/dist..." -ForegroundColor Yellow
if (Test-Path dist) { Remove-Item -Recurse -Force dist }
New-Item -ItemType Directory -Path dist\touch, dist\pkg, dist\icons | Out-Null
Copy-Item index.html, touch.html, style.css, ui.js, ui.js.map, manifest.webmanifest, sw.js dist\
Copy-Item touch\style.css, touch\app.js, touch\app.js.map dist\touch\
Copy-Item icons\icon-192.png, icons\icon-512.png dist\icons\
Copy-Item -Recurse pkg\* dist\pkg\
Write-Host "✓ web/dist assembled" -ForegroundColor Green

# Step 3: Build extension
Write-Host "`n[3/4] Building extension..." -ForegroundColor Yellow
Set-Location ..\editors\vscode
npm run build
if ($LASTEXITCODE -ne 0) { throw "Extension build failed" }

# Step 4: Package VSIX
Write-Host "`n[4/4] Packaging VSIX..." -ForegroundColor Yellow
if (Test-Path "confy-vscode-0.1.0.vsix") { Remove-Item "confy-vscode-0.1.0.vsix" }
npm run package
if ($LASTEXITCODE -ne 0) { throw "Packaging failed" }

Write-Host "`n=== Build Complete ===" -ForegroundColor Green
Write-Host "VSIX file: $PWD\confy-vscode-0.1.0.vsix" -ForegroundColor Cyan
