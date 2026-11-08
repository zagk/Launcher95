# Generates assets/launcher95.ico (multi-size, 32-bit BMP entries).
# Requires Windows PowerShell 5.1+ with System.Drawing.

param(
    [string]$OutFile = (Join-Path $PSScriptRoot '..\assets\launcher95.ico')
)

Add-Type -AssemblyName System.Drawing

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::SingleBitPerPixelGridFit
    $g.Clear([System.Drawing.Color]::Transparent)

    $m = [Math]::Max(1, [int]($size * 0.06))
    $w = $size - 2 * $m
    $h = $size - 2 * $m
    if ($w -lt 8) { $w = 8 }
    if ($h -lt 8) { $h = 8 }
    $x = $m
    $y = $m
    $titleH = [Math]::Max(2, [int]($h * 0.18))

    $border = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 0, 0, 0))
    $body = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 192, 192, 192))
    $face = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 223, 223, 223))
    $navy = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 0, 0, 128))
    $white = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
    $teal = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 0, 128, 128))

    # Outer window frame
    $g.FillRectangle($border, $x, $y, $w, $h)
    $g.FillRectangle($body, $x + 1, $y + 1, $w - 2, $h - 2)
    # Title bar
    $g.FillRectangle($navy, $x + 1, $y + 1, $w - 2, $titleH)
    # Fake title text pixels
    $dot = [Math]::Max(1, [int]($size / 16))
    $g.FillRectangle($white, $x + 3 * $dot, $y + 2 * $dot, [Math]::Max(2, $w / 3), $dot)
    # Window buttons
    $g.FillRectangle($face, $x + $w - 4 * $dot, $y + 2 * $dot, $dot, $dot)
    # Client area
    $cy = $y + 1 + $titleH
    $ch = $h - 2 - $titleH
    $g.FillRectangle($white, $x + 1, $cy, $w - 2, $ch)
    # Screen
    $sx = $x + [int]($w * 0.18)
    $sy = $cy + [int]($ch * 0.18)
    $sw = $w - [int]($w * 0.36)
    $sh = $ch - [int]($ch * 0.36)
    if ($sw -ge 4 -and $sh -ge 4) {
        $g.FillRectangle($teal, $sx, $sy, $sw, $sh)
        $g.DrawRectangle((New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 0, 0, 0), 1)), ($sx - 1), ($sy - 1), ($sw + 1), ($sh + 1))
        if ($size -ge 32) {
            $fontSize = [Math]::Max(6, [int]($size * 0.28))
            $font = New-Object System.Drawing.Font('Tahoma', $fontSize, ([System.Drawing.FontStyle]::Bold), [System.Drawing.GraphicsUnit]::Pixel)
            $fmt = New-Object System.Drawing.StringFormat
            $fmt.Alignment = [System.Drawing.StringAlignment]::Center
            $fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
            $rect = New-Object System.Drawing.RectangleF($sx, $sy, $sw, $sh)
            $g.DrawString('95', $font, $white, $rect, $fmt)
            $font.Dispose()
            $fmt.Dispose()
        }
    }

    $border.Dispose(); $body.Dispose(); $face.Dispose(); $navy.Dispose(); $white.Dispose(); $teal.Dispose()
    $g.Dispose()
    return $bmp
}

function Get-IconImageBytes([System.Drawing.Bitmap]$bmp) {
    $w = $bmp.Width
    $h = $bmp.Height
    $rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $data.Stride
    $raw = New-Object byte[] ($stride * $h)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $raw, 0, $raw.Length)
    $bmp.UnlockBits($data)

    $maskStride = [int]([Math]::Floor(($w + 31) / 32) * 4)
    $imageSize = 40 + $w * $h * 4 + $maskStride * $h
    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)

    # BITMAPINFOHEADER
    $bw.Write([uint32]40)
    $bw.Write([int32]$w)
    $bw.Write([int32]($h * 2))
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]0)
    $bw.Write([uint32]($w * $h * 4))
    $bw.Write([int32]0); $bw.Write([int32]0)
    $bw.Write([uint32]0); $bw.Write([uint32]0)

    # XOR bitmap, bottom-up BGRA
    for ($row = $h - 1; $row -ge 0; $row--) {
        $bw.Write($raw, $row * $stride, $w * 4)
    }
    # AND mask (all zero = opaque)
    $zero = New-Object byte[] ($maskStride * $h)
    $bw.Write($zero, 0, $zero.Length)

    $bw.Flush()
    $bytes = $ms.ToArray()
    $bw.Dispose(); $ms.Dispose()
    return $bytes
}

$sizes = @(16, 32, 48, 64, 128, 256)
$images = @()
foreach ($size in $sizes) {
    $bmp = New-IconBitmap $size
    $images += ,@($size, (Get-IconImageBytes $bmp))
    $bmp.Dispose()
}

$dir = Split-Path -Parent $OutFile
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }

$fs = [System.IO.File]::Create($OutFile)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$images.Count)

$offset = 6 + 16 * $images.Count
foreach ($image in $images) {
    $size = $image[0]
    $bytes = $image[1]
    $dim = if ($size -ge 256) { 0 } else { $size }
    $bw.Write([byte]$dim)
    $bw.Write([byte]$dim)
    $bw.Write([byte]0)
    $bw.Write([byte]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]$bytes.Length)
    $bw.Write([uint32]$offset)
    $offset += $bytes.Length
}
foreach ($image in $images) {
    $bw.Write($image[1], 0, $image[1].Length)
}
$bw.Flush()
$bw.Dispose(); $fs.Dispose()

Write-Host "Wrote $OutFile ($((Get-Item $OutFile).Length) bytes)"
