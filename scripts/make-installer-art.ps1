# Regenerate the NSIS installer artwork from the app icon.
#
#   powershell -ExecutionPolicy Bypass -File scripts\make-installer-art.ps1
#
# Output (desktop/src-tauri/installer/):
#   header.bmp   150x57   drawn in the wizard header band, to the right of the page title
#   sidebar.bmp  164x314  the left strip of the welcome and finish pages
#
# Both MUST be 24-bit BMP at exactly those sizes. NSIS/MUI2 does not scale them,
# and a 32-bit BMP (what GDI+ writes by default, alpha channel included) renders
# as garbage in the wizard. Hence the explicit Format24bppRgb bitmaps below.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root      = Split-Path -Parent $PSScriptRoot
$iconPath  = Join-Path $root 'desktop\src-tauri\icons\icon.png'
$outDir    = Join-Path $root 'desktop\src-tauri\installer'

if (-not (Test-Path $iconPath)) { throw "icon not found: $iconPath" }
if (-not (Test-Path $outDir))   { New-Item -ItemType Directory -Path $outDir | Out-Null }

# Palette lifted from desktop/src/styles.css so the installer matches the app.
$bgTop    = [System.Drawing.Color]::FromArgb(0x1B, 0x15, 0x11)
$bgBottom = [System.Drawing.Color]::FromArgb(0x0D, 0x0B, 0x0A)
$ink      = [System.Drawing.Color]::FromArgb(0xF5, 0xF1, 0xEC)
$inkMute  = [System.Drawing.Color]::FromArgb(0x8A, 0x81, 0x7A)
$accent   = [System.Drawing.Color]::FromArgb(0xE6, 0x8A, 0x3D)
$rule     = [System.Drawing.Color]::FromArgb(0x3A, 0x2B, 0x1E)

$icon = [System.Drawing.Image]::FromFile($iconPath)

function New-Canvas([int]$w, [int]$h) {
    # Format24bppRgb, not the default 32bpp: see the header comment.
    $bmp = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    return @($bmp, $g)
}

function Save-Bmp($bmp, [string]$name) {
    $path = Join-Path $outDir $name
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Bmp)
    $len = (Get-Item $path).Length
    Write-Host ("  {0}  {1}x{2}  {3} bytes" -f $name, $bmp.Width, $bmp.Height, $len)
}

# ---------------------------------------------------------------- sidebar ----
# 164x314, dark, matching the icon's own background so the mark sits flush.
$w, $h = 164, 314
$bmp, $g = New-Canvas $w $h

$gradRect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush($gradRect, $bgTop, $bgBottom, 90.0)
$g.FillRectangle($grad, $gradRect)
$grad.Dispose()

# Text on a dark background: grid-fit antialiasing, no ClearType colour fringes.
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit

$markSize = 76
$g.DrawImage($icon, [int](($w - $markSize) / 2), 62, $markSize, $markSize)

$fmt = New-Object System.Drawing.StringFormat
$fmt.Alignment = [System.Drawing.StringAlignment]::Center

$wordmark = New-Object System.Drawing.Font('Segoe UI Semibold', 21, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$brushInk = New-Object System.Drawing.SolidBrush($ink)
$g.DrawString('Sotto', $wordmark, $brushInk, [single]($w / 2), 156, $fmt)

$pen = New-Object System.Drawing.Pen($rule, 1)
$g.DrawLine($pen, ($w / 2 - 20), 190, ($w / 2 + 20), 190)

$tag = New-Object System.Drawing.Font('Segoe UI', 11, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$brushMute = New-Object System.Drawing.SolidBrush($inkMute)
$g.DrawString('speech to text', $tag, $brushMute, [single]($w / 2), 200, $fmt)

# Accent hairline along the inner edge, where the strip meets the white page.
$penAccent = New-Object System.Drawing.Pen($accent, 2)
$g.DrawLine($penAccent, ($w - 1), 0, ($w - 1), $h)

$g.Dispose()
Save-Bmp $bmp 'sidebar.bmp'
$bmp.Dispose()

# ----------------------------------------------------------------- header ----
# 150x57 on the wizard's white header band, so the background must be white.
$w, $h = 150, 57
$bmp, $g = New-Canvas $w $h
$g.Clear([System.Drawing.Color]::White)

$markSize = 38
$g.DrawImage($icon, ($w - $markSize - 10), [int](($h - $markSize) / 2), $markSize, $markSize)

$g.Dispose()
Save-Bmp $bmp 'header.bmp'
$bmp.Dispose()

$icon.Dispose()
Write-Host 'installer art regenerated'
