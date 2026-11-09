use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

/// Bumped whenever the extraction logic changes so old cache files are ignored.
const CACHE_VERSION: &str = "v3";

/// Deterministic FNV-1a hash (unlike `DefaultHasher`, whose keys are random per run).
/// Used for entry IDs and the icon cache so lookups are stable across launches.
fn stable_hash(parts: &[&str]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for part in parts {
        for &byte in part.as_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Loads native Windows shell icons lazily and caches them as egui textures.
///
/// Shortcuts are resolved to their real target first, so a dropped `.lnk` shows
/// the icon of the program it points at instead of a generic shortcut glyph.
/// Extracted 32-bit RGBA data is also written to `Data/icons`.
pub struct IconCache {
    cache_dir: PathBuf,
    textures: HashMap<String, TextureHandle>,
    missing: HashSet<String>,
}

impl IconCache {
    pub fn new(data_dir: &Path) -> Self {
        let cache_dir = data_dir.join("icons");
        let _ = fs::create_dir_all(&cache_dir);
        // Drop cache files written by older extraction logic.
        if let Ok(entries) = fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".icon") && !name.starts_with(&format!("{CACHE_VERSION}-")) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Self {
            cache_dir,
            textures: HashMap::new(),
            missing: HashSet::new(),
        }
    }

    /// Returns a texture for `path`, extracting it at roughly `pixel_size` when needed.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        pixel_size: u32,
    ) -> Option<TextureHandle> {
        let pixel_size = pixel_size.clamp(16, 256);
        let key = format!("{}|{pixel_size}", path.to_ascii_lowercase());
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.clone());
        }
        if self.missing.contains(&key) {
            return None;
        }

        match self.load_or_extract(path, pixel_size) {
            Some((width, height, rgba)) => {
                let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
                let texture =
                    ctx.load_texture(format!("icon:{key}"), image, TextureOptions::LINEAR);
                self.textures.insert(key, texture.clone());
                Some(texture)
            }
            None => {
                self.missing.insert(key);
                None
            }
        }
    }

    fn load_or_extract(&self, path: &str, pixel_size: u32) -> Option<(u32, u32, Vec<u8>)> {
        let cache_file = self.cache_file(path, pixel_size);
        if let Ok(bytes) = fs::read(&cache_file) {
            if let Some(image) = decode_cache(&bytes) {
                return Some(image);
            }
        }

        #[cfg(windows)]
        {
            let (width, height, rgba) = extract_rgba(path, pixel_size)?;
            let mut bytes = Vec::with_capacity(8 + rgba.len());
            bytes.extend_from_slice(&width.to_le_bytes());
            bytes.extend_from_slice(&height.to_le_bytes());
            bytes.extend_from_slice(&rgba);
            let _ = fs::write(&cache_file, bytes);
            Some((width, height, rgba))
        }
        #[cfg(not(windows))]
        {
            let _ = (path, pixel_size);
            None
        }
    }

    fn cache_file(&self, path: &str, pixel_size: u32) -> PathBuf {
        let mut key = String::with_capacity(path.len() + 24);
        key.push_str(CACHE_VERSION);
        key.push('|');
        key.push_str(&path.to_ascii_lowercase());
        key.push('|');
        key.push_str(&pixel_size.to_string());
        if let Ok(metadata) = fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = modified.duration_since(std::time::UNIX_EPOCH) {
                    key.push('|');
                    key.push_str(&age.as_secs().to_string());
                }
            }
        }
        let hash = stable_hash(&[&key]);
        self.cache_dir
            .join(format!("{CACHE_VERSION}-{hash:016x}.icon"))
    }
}

fn decode_cache(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() < 8 {
        return None;
    }
    let width = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let expected = 8 + width as usize * height as usize * 4;
    if bytes.len() != expected {
        return None;
    }
    Some((width, height, bytes[8..].to_vec()))
}

/// A concrete icon resource: a file plus an index inside it.
struct IconSource {
    file: PathBuf,
    index: i32,
}

#[cfg(windows)]
fn extract_rgba(path: &str, pixel_size: u32) -> Option<(u32, u32, Vec<u8>)> {
    if is_extension(Path::new(path), "lnk") || is_extension(Path::new(path), "url") {
        let mut visited = HashSet::new();
        return shortcut_icon(path, pixel_size, &mut visited, 0);
    }

    // Plain executables are read directly so every size stays sharp.
    if is_extension(Path::new(path), "exe") {
        if let Some(image) = extract_from_file(Path::new(path), 0, pixel_size) {
            return Some(image);
        }
    }

    // Other files use the shell's association icon.
    if let Some(image) = shell_icon(path, pixel_size) {
        return Some(image);
    }

    // Missing files still have a type icon; ask the shell by extension only.
    shell_type_icon(path, pixel_size)
}

/// Resolves the icon of a shortcut.
///
/// Order matters:
/// 1. An explicit `IconLocation` that exists is authoritative.
/// 2. A declared-but-missing icon location means we should not read the target
///    (web-app shortcuts point at a generic host executable), so ask the shell.
/// 3. Otherwise read the target's embedded icon, then fall back to the shell.
#[cfg(windows)]
fn shortcut_icon(
    path: &str,
    pixel_size: u32,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Option<(u32, u32, Vec<u8>)> {
    if depth > 8 {
        return shell_type_icon(path, pixel_size);
    }
    let key = path.to_ascii_lowercase();
    if !visited.insert(key) {
        // Circular shortcut chain (A -> B -> A): bail out to the shell icon.
        return shell_type_icon(path, pixel_size);
    }
    if is_extension(Path::new(path), "url") {
        return internet_shortcut_icon(Path::new(path), pixel_size);
    }

    let (target, declared) = read_shortcut_or_none(path);

    if let Some((file, index)) = &declared {
        let file = PathBuf::from(expand_env(&file.to_string_lossy()));
        if file.exists() {
            if let Some(image) = extract_from_file(&file, *index, pixel_size) {
                return Some(image);
            }
        } else if let Some(image) = shell_icon(path, pixel_size) {
            return Some(image);
        }
    }

    if let Some(target) = target {
        if target.exists() {
            if is_extension(&target, "lnk") || is_extension(&target, "url") {
                if let Some(image) = shortcut_icon(
                    &target.to_string_lossy(),
                    pixel_size,
                    visited,
                    depth + 1,
                ) {
                    return Some(image);
                }
            } else if let Some(image) = extract_from_file(&target, 0, pixel_size) {
                return Some(image);
            } else if let Some(image) = shell_icon(&target.to_string_lossy(), pixel_size) {
                return Some(image);
            }
        }
    }

    if let Some(image) = shell_icon(path, pixel_size) {
        return Some(image);
    }
    shell_type_icon(path, pixel_size)
}

/// Split form of [`read_shortcut`] that never fails the whole lookup.
#[cfg(windows)]
fn read_shortcut_or_none(link: &str) -> (Option<PathBuf>, Option<(PathBuf, i32)>) {
    read_shortcut(Path::new(link)).unwrap_or((None, None))
}

/// Resolves an Internet shortcut (`IconFile=` / `IconIndex=`).
#[cfg(windows)]
fn internet_shortcut_icon(path: &Path, pixel_size: u32) -> Option<(u32, u32, Vec<u8>)> {
    if let Some(source) = read_url_icon(path) {
        if let Some(image) = extract_from_file(&source.file, source.index, pixel_size) {
            return Some(image);
        }
    }
    if let Some(image) = shell_icon(&path.to_string_lossy(), pixel_size) {
        return Some(image);
    }
    shell_type_icon(&path.to_string_lossy(), pixel_size)
}

/// Resolves the icon by file extension even when the file does not exist,
/// which keeps tiles for broken shortcuts from turning into bare glyphs.
#[cfg(windows)]
fn shell_type_icon(path: &str, pixel_size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;

    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    let wide = to_wide_os(std::ffi::OsStr::new(path));
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<SHFILEINFOW>() as u32;

    unsafe {
        let r = SHGetFileInfoW(
            wide.as_ptr(),
            FILE_ATTRIBUTE_NORMAL,
            &mut info,
            size,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        );
        if r == 0 || info.hIcon.is_null() {
            return None;
        }
        let hicon = info.hIcon;
        let result = hicon_to_rgba(hicon, pixel_size);
        DestroyIcon(hicon);
        result
    }
}

/// Returns the icon resource declared by a shortcut, if any.
///
/// Only used by tests; the production path is [`shortcut_icon`], which also
/// falls back to the shell.
#[cfg(all(windows, test))]
fn icon_source(path: &str) -> Option<IconSource> {
    let link = Path::new(path);
    if is_extension(link, "url") {
        return read_url_icon(link);
    }
    if is_extension(link, "lnk") {
        let (target, icon) = read_shortcut_or_none(path);
        if let Some((file, index)) = icon {
            let file = PathBuf::from(expand_env(&file.to_string_lossy()));
            if file.exists() {
                return Some(IconSource { file, index });
            }
        }
        let target = target?;
        if !target.exists() || is_extension(&target, "lnk") || is_extension(&target, "url") {
            return None;
        }
        return Some(IconSource {
            file: target,
            index: 0,
        });
    }
    None
}

#[cfg(windows)]
fn is_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(wanted))
}

#[cfg(windows)]
fn read_shortcut(link: &Path) -> Option<(Option<PathBuf>, Option<(PathBuf, i32)>)> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    unsafe {
        // The UI thread is normally already apartment-threaded (winit OLE
        // drag/drop). A conflicting model still works for these read calls.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = shell_link.cast().ok()?;
        let wide = to_wide_os(link.as_os_str());
        persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut path_buffer = vec![0u16; 1024];
        let target = shell_link
            .GetPath(&mut path_buffer, std::ptr::null_mut(), 0)
            .ok()
            .map(|()| from_wide(&path_buffer))
            .filter(|text| !text.is_empty())
            .map(PathBuf::from);

        let mut icon_buffer = vec![0u16; 1024];
        let mut icon_index = 0i32;
        let icon = shell_link
            .GetIconLocation(&mut icon_buffer, &mut icon_index)
            .ok()
            .map(|()| from_wide(&icon_buffer))
            .filter(|text| !text.is_empty())
            .map(|text| (PathBuf::from(text), icon_index));

        Some((target, icon))
    }
}

/// Reads `IconFile=` / `IconIndex=` from an Internet shortcut.
#[cfg(windows)]
fn read_url_icon(path: &Path) -> Option<IconSource> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut icon_file: Option<String> = None;
    let mut icon_index = 0i32;

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "iconfile" => icon_file = Some(value.trim().to_string()),
            "iconindex" => icon_index = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }

    let file = PathBuf::from(expand_env(icon_file?.as_str()));
    if !file.exists() {
        return None;
    }
    Some(IconSource {
        file,
        index: icon_index,
    })
}

/// Expands `%SystemRoot%`-style variables in shortcut icon paths.
fn expand_env(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            result.push_str(&rest[start..]);
            return result;
        };
        let name = &after[..end];
        match std::env::var(name) {
            Ok(value) if !name.is_empty() => result.push_str(&value),
            _ => {
                result.push('%');
                result.push_str(name);
                result.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(windows)]
fn extract_from_file(
    file: &Path,
    index: i32,
    pixel_size: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, PrivateExtractIconsW};

    if !file.exists() {
        return None;
    }
    let wide = to_wide_os(file.as_os_str());

    unsafe {
        let mut hicon = std::ptr::null_mut();
        let mut icon_id = 0u32;
        let count = PrivateExtractIconsW(
            wide.as_ptr(),
            index,
            pixel_size as i32,
            pixel_size as i32,
            &mut hicon,
            &mut icon_id,
            1,
            0,
        );
        if count == 0 || hicon.is_null() {
            return None;
        }
        let result = hicon_to_rgba(hicon, pixel_size);
        DestroyIcon(hicon);
        result
    }
}

/// Asks the shell for the icon it would show for this file, preferring the
/// high resolution image lists so large icon sizes stay crisp.
#[cfg(windows)]
fn shell_icon(path: &str, pixel_size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_SYSICONINDEX,
        SHIL_EXTRALARGE, SHIL_JUMBO, SHIL_LARGE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    let wide = to_wide_os(std::ffi::OsStr::new(path));
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<SHFILEINFOW>() as u32;

    unsafe {
        let r = SHGetFileInfoW(wide.as_ptr(), 0, &mut info, size, SHGFI_SYSICONINDEX);
        if r != 0 {
            let list = if pixel_size >= 64 {
                SHIL_JUMBO
            } else if pixel_size >= 40 {
                SHIL_EXTRALARGE
            } else {
                SHIL_LARGE
            };
            if let Some(hicon) = image_list_icon(list as i32, info.iIcon as i32) {
                let result = hicon_to_rgba(hicon, pixel_size);
                DestroyIcon(hicon);
                if result.is_some() {
                    return result;
                }
            }
        }

        let mut info: SHFILEINFOW = std::mem::zeroed();
        let r = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            size,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || info.hIcon.is_null() {
            return None;
        }
        let hicon: HICON = info.hIcon;
        let result = hicon_to_rgba(hicon, pixel_size);
        DestroyIcon(hicon);
        result
    }
}

/// Releases an `IUnknown`-derived COM pointer obtained as a raw pointer.
#[cfg(windows)]
unsafe fn release_com(ptr: *mut std::ffi::c_void) {
    if ptr.is_null() {
        return;
    }
    let vtable = *(ptr as *const *const usize);
    if vtable.is_null() {
        return;
    }
    let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
        std::mem::transmute(*vtable.add(2));
    release(ptr);
}

/// Extracts one image from a shell system image list as an `HICON`.
#[cfg(windows)]
unsafe fn image_list_icon(
    list: i32,
    index: i32,
) -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    use std::ffi::c_void;
    use windows_sys::Win32::UI::Controls::ImageList_GetIcon;
    use windows_sys::Win32::UI::Shell::SHGetImageList;

    // IID_IImageList
    const IID_IMAGE_LIST: windows_sys::core::GUID =
        windows_sys::core::GUID::from_u128(0x46EB5926_582E_4017_9FDF_E8998DAA0950);
    const ILD_TRANSPARENT: u32 = 1;

    let mut raw: *mut c_void = std::ptr::null_mut();
    if SHGetImageList(list, &IID_IMAGE_LIST, &mut raw) < 0 || raw.is_null() {
        return None;
    }
    let hicon = ImageList_GetIcon(raw as _, index, ILD_TRANSPARENT);
    release_com(raw);
    if hicon.is_null() {
        return None;
    }
    Some(hicon)
}

#[cfg(windows)]
fn to_wide_os(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn from_wide(buffer: &[u16]) -> String {
    let length = buffer
        .iter()
        .position(|&value| value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

#[cfg(windows)]
unsafe fn hicon_to_rgba(
    hicon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    fallback_size: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon, &mut info) == 0 {
        return None;
    }

    let cleanup = |bitmap: *mut c_void| {
        if !bitmap.is_null() {
            DeleteObject(bitmap as HGDIOBJ);
        }
    };

    if info.hbmColor.is_null() {
        cleanup(info.hbmMask);
        return None;
    }

    let hdc = GetDC(std::ptr::null_mut());
    if hdc.is_null() {
        cleanup(info.hbmColor);
        cleanup(info.hbmMask);
        return None;
    }

    let mut bitmap: BITMAP = std::mem::zeroed();
    let object_ok = GetObjectW(
        info.hbmColor as HGDIOBJ,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bitmap as *mut BITMAP as *mut c_void,
    );
    if object_ok == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
        let _ = ReleaseDC(std::ptr::null_mut(), hdc);
        cleanup(info.hbmColor);
        cleanup(info.hbmMask);
        return None;
    }

    let width = bitmap.bmWidth as u32;
    let height = bitmap.bmHeight as u32;
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width as i32;
    bmi.bmiHeader.biHeight = -(height as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let lines = GetDIBits(
        hdc,
        info.hbmColor,
        0,
        height,
        rgba.as_mut_ptr() as *mut c_void,
        &mut bmi,
        DIB_RGB_COLORS,
    );

    let mut result = None;
    if lines != 0 {
        let mut has_alpha = false;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2); // BGRA -> RGBA
            has_alpha |= pixel[3] != 0;
        }

        if !has_alpha {
            apply_mask_alpha(hdc, info.hbmMask, width, height, &mut rgba);
        }
        result = Some((width, height, rgba));
    }

    let _ = ReleaseDC(std::ptr::null_mut(), hdc);
    cleanup(info.hbmColor);
    cleanup(info.hbmMask);

    if result.is_none() {
        return draw_icon_fallback(hicon, fallback_size);
    }
    result
}

#[cfg(windows)]
unsafe fn apply_mask_alpha(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    hbm_mask: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    width: u32,
    height: u32,
    rgba: &mut [u8],
) {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    let stride = ((width as usize + 31) / 32) * 4;
    let mut mask = vec![0u8; stride * height as usize];
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width as i32;
    bmi.bmiHeader.biHeight = -(height as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 1;
    bmi.bmiHeader.biCompression = BI_RGB;

    if hbm_mask.is_null()
        || GetDIBits(
            hdc,
            hbm_mask,
            0,
            height,
            mask.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        ) == 0
    {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        return;
    }

    for y in 0..height as usize {
        for x in 0..width as usize {
            let bit = (mask[y * stride + x / 8] >> (7 - (x % 8))) & 1;
            rgba[(y * width as usize + x) * 4 + 3] = if bit == 1 { 0 } else { 255 };
        }
    }
}

/// Last-resort path for monochrome or unusual icons.
#[cfg(windows)]
unsafe fn draw_icon_fallback(
    hicon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    size: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

    let size = size.clamp(16, 256);
    let hdc = CreateCompatibleDC(std::ptr::null_mut());
    if hdc.is_null() {
        return None;
    }

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = size as i32;
    bmi.bmiHeader.biHeight = -(size as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(
        hdc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits,
        std::ptr::null_mut(),
        0,
    );
    if bitmap.is_null() || bits.is_null() {
        let _ = DeleteDC(hdc);
        return None;
    }

    let old = SelectObject(hdc, bitmap as HGDIOBJ);
    let drawn = DrawIconEx(
        hdc,
        0,
        0,
        hicon,
        size as i32,
        size as i32,
        0,
        std::ptr::null_mut(),
        DI_NORMAL,
    );
    SelectObject(hdc, old);
    let _ = DeleteDC(hdc);

    let mut result = None;
    if drawn != 0 {
        let mut rgba =
            std::slice::from_raw_parts(bits as *const u8, size as usize * size as usize * 4)
                .to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            if pixel[3] == 0 && (pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0) {
                pixel[3] = 255;
            }
        }
        result = Some((size, size, rgba));
    }
    let _ = DeleteObject(bitmap as HGDIOBJ);
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn extracts_notepad_icon() {
        let path = r"C:\Windows\System32\notepad.exe";
        if !Path::new(path).exists() {
            return;
        }
        let (width, height, rgba) = extract_rgba(path, 48).expect("icon");
        assert!(width >= 16 && height >= 16);
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn expands_environment_variables() {
        let expanded = expand_env(r"%SystemRoot%\system32\shell32.dll");
        assert!(!expanded.contains('%'));
        assert!(expanded.to_ascii_lowercase().contains("shell32.dll"));
    }

    #[test]
    fn shortcut_resolves_to_target_icon() {
        let user_root = dirs::config_dir()
            .map(|dir| dir.join("Microsoft/Windows/Start Menu/Programs"));
        let Some(user_root) = user_root else {
            return;
        };
        let links = walk_links(&user_root, 2);
        let mut resolved = 0usize;
        for link in &links {
            let Some(source) = icon_source(&link.to_string_lossy()) else {
                continue;
            };
            if !source.file.exists() {
                continue;
            }
            assert!(
                !source
                    .file
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".lnk"),
                "shortcut should resolve past the .lnk indirection"
            );
            resolved += 1;
        }
        assert!(
            links.is_empty() || resolved > 0,
            "at least one Start Menu shortcut should resolve to a real target"
        );
    }

    #[test]
    fn shortcut_icon_matches_target_icon() {
        let user_root = dirs::config_dir()
            .map(|dir| dir.join("Microsoft/Windows/Start Menu/Programs"));
        let Some(user_root) = user_root else {
            return;
        };
        for link in walk_links(&user_root, 2) {
            let text = link.to_string_lossy().to_string();
            let Some(source) = icon_source(&text) else {
                continue;
            };
            if !is_extension(&source.file, "exe") {
                continue;
            }
            let direct = extract_from_file(&source.file, source.index, 48);
            let via_link = extract_rgba(&text, 48);
            if let (Some(direct), Some(via_link)) = (direct, via_link) {
                assert_eq!(
                    direct, via_link,
                    "icon through .lnk should equal the target exe icon"
                );
                return;
            }
        }
    }

    fn walk_links(root: &Path, depth: usize) -> Vec<PathBuf> {
        let mut found = Vec::new();
        if depth == 0 {
            return found;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk_links(&path, depth - 1));
            } else if is_extension(&path, "lnk") {
                found.push(path);
            }
        }
        found
    }
}
