//! Desktop-entry icons, independent of the hidden client and the spine title.
use std::collections::HashSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use gio::glib::{Bytes, KeyFile, KeyFileFlags};
use gio::prelude::*;
use pangocairo::cairo::{self, ImageSurface};

#[derive(Debug, Default)]
pub(super) struct AppIconCache {
    key: Option<(String, i32)>,
    surface: Option<ImageSurface>,
}

impl AppIconCache {
    pub fn clear(&mut self) {
        self.key = None;
        self.surface = None;
    }

    pub fn get(&mut self, app_id: &str, pixels: i32) -> Option<ImageSurface> {
        self.get_with(app_id, pixels, || {
            resolve(app_id, pixels).and_then(|path| decode(&path, pixels).ok())
        })
    }

    fn get_with(
        &mut self,
        app_id: &str,
        pixels: i32,
        load: impl FnOnce() -> Option<ImageSurface>,
    ) -> Option<ImageSurface> {
        let key = (app_id.to_owned(), pixels);
        if self.key.as_ref() != Some(&key) {
            self.surface = load();
            // Cache failures too. Reload or a new app ID/size explicitly invalidates them.
            self.key = Some(key);
        }
        self.surface.clone()
    }
}

fn resolve(app_id: &str, pixels: i32) -> Option<PathBuf> {
    if !single_name(app_id) {
        return None;
    }
    let desktop_id = if app_id.ends_with(".desktop") {
        app_id.to_owned()
    } else {
        format!("{app_id}.desktop")
    };
    let icon = gio::DesktopAppInfo::new(&desktop_id)
        .and_then(|info| info.string("Icon"))
        .map(|value| value.to_string())
        .unwrap_or_else(|| app_id.trim_end_matches(".desktop").to_owned());
    if Path::new(&icon).is_absolute() {
        return Some(icon.into());
    }
    if !single_name(&icon) {
        return None;
    }
    let dirs = directories::BaseDirs::new()?;
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
    let data_dirs = std::iter::once(dirs.data_dir().to_path_buf())
        .chain(data_dirs.into_iter().filter(|path| path.is_absolute()))
        .collect::<Vec<_>>();
    let roots = std::iter::once(dirs.home_dir().join(".icons"))
        .chain(data_dirs.iter().map(|path| path.join("icons")))
        .collect::<Vec<_>>();
    let theme = default_theme(&dirs).unwrap_or_else(|| "hicolor".into());
    let mut visited = HashSet::new();
    lookup_theme(&roots, &theme, &icon, pixels, &mut visited)
        .or_else(|| lookup_theme(&roots, "hicolor", &icon, pixels, &mut visited))
        .or_else(|| {
            data_dirs
                .iter()
                .find_map(|path| icon_file(&path.join("pixmaps"), &icon))
        })
}

fn single_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\0')
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
}

fn relative_directory(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn key_file(path: &Path) -> Option<KeyFile> {
    let data = String::from_utf8(read_bounded(path, 1024 * 1024).ok()?).ok()?;
    let file = KeyFile::new();
    file.load_from_data(&data, KeyFileFlags::NONE).ok()?;
    Some(file)
}

fn default_theme(dirs: &directories::BaseDirs) -> Option<String> {
    for version in ["gtk-4.0", "gtk-3.0"] {
        if let Some(theme) = key_file(&dirs.config_dir().join(version).join("settings.ini"))
            .and_then(|file| file.string("Settings", "gtk-icon-theme-name").ok())
            .filter(|theme| single_name(theme))
        {
            return Some(theme.to_string());
        }
    }
    // Missing schemas are normal on non-GNOME systems; never call Settings::new blindly.
    let schema =
        gio::SettingsSchemaSource::default()?.lookup("org.gnome.desktop.interface", true)?;
    if !schema.has_key("icon-theme") {
        return None;
    }
    let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
    let theme = settings.string("icon-theme");
    single_name(&theme).then(|| theme.to_string())
}

fn icon_file(directory: &Path, name: &str) -> Option<PathBuf> {
    let exact_extension = matches!(
        Path::new(name).extension().and_then(|s| s.to_str()),
        Some("png" | "svg" | "xpm")
    );
    if exact_extension {
        let path = directory.join(name);
        return path.is_file().then_some(path);
    }
    ["png", "svg", "xpm"].into_iter().find_map(|extension| {
        let path = directory.join(format!("{name}.{extension}"));
        path.is_file().then_some(path)
    })
}

fn lookup_theme(
    roots: &[PathBuf],
    theme: &str,
    icon: &str,
    pixels: i32,
    visited: &mut HashSet<String>,
) -> Option<PathBuf> {
    if !single_name(theme) || visited.len() >= 64 || !visited.insert(theme.to_owned()) {
        return None;
    }
    // The first index describes the theme across every base directory.
    let index = roots
        .iter()
        .find_map(|root| key_file(&root.join(theme).join("index.theme")))?;
    let directories = index.string("Icon Theme", "Directories").ok()?;
    let scaled = index
        .string("Icon Theme", "ScaledDirectories")
        .unwrap_or_default();
    let mut closest: Option<(i64, PathBuf)> = None;
    for directory in directories
        .split(',')
        .chain(scaled.split(','))
        .map(str::trim)
        .filter(|d| relative_directory(d))
    {
        let Some(distance) = size_distance(&index, directory, pixels) else {
            continue;
        };
        for root in roots {
            let Some(path) = icon_file(&root.join(theme).join(directory), icon) else {
                continue;
            };
            if distance == 0 {
                return Some(path);
            }
            if closest
                .as_ref()
                .is_none_or(|(current, _)| distance < *current)
            {
                closest = Some((distance, path));
            }
        }
    }
    if let Some((_, path)) = closest {
        return Some(path);
    }
    index
        .string("Icon Theme", "Inherits")
        .ok()?
        .split(',')
        .map(str::trim)
        .find_map(|parent| lookup_theme(roots, parent, icon, pixels, visited))
}

fn size_distance(index: &KeyFile, directory: &str, pixels: i32) -> Option<i64> {
    let size = i64::from(index.integer(directory, "Size").ok()?);
    let scale = i64::from(index.integer(directory, "Scale").unwrap_or(1));
    if size <= 0 || scale <= 0 {
        return None;
    }
    let kind = index
        .string(directory, "Type")
        .unwrap_or_else(|_| "Threshold".into());
    let (min, max) = match kind.as_str() {
        "Fixed" => (size, size),
        "Scalable" => (
            i64::from(index.integer(directory, "MinSize").unwrap_or(size as i32)),
            i64::from(index.integer(directory, "MaxSize").unwrap_or(size as i32)),
        ),
        "Threshold" => {
            let threshold = i64::from(index.integer(directory, "Threshold").unwrap_or(2)).max(0);
            (size - threshold, size + threshold)
        }
        _ => return None,
    };
    let pixels = i64::from(pixels);
    Some((min * scale - pixels).max(pixels - max * scale).max(0))
}

fn decode(path: &Path, pixels: i32) -> anyhow::Result<ImageSurface> {
    let data = Bytes::from_owned(read_bounded(path, 16 * 1024 * 1024)?);
    let stream = gio::MemoryInputStream::from_bytes(&data);
    let pixels = pixels.clamp(1, 256);
    let pixbuf = gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        pixels,
        pixels,
        true,
        None::<&gio::Cancellable>,
    )?;
    let (w, h) = (pixbuf.width(), pixbuf.height());
    anyhow::ensure!(
        w > 0 && h > 0 && w <= pixels && h <= pixels && pixbuf.bits_per_sample() == 8,
        "invalid icon dimensions"
    );
    let channels = pixbuf.n_channels() as usize;
    anyhow::ensure!(matches!(channels, 3 | 4), "invalid icon channels");
    let stride = pixbuf.rowstride() as usize;
    let data = pixbuf.read_pixel_bytes();
    let mut surface = ImageSurface::create(cairo::Format::ARgb32, w, h)?;
    let output_stride = surface.stride() as usize;
    {
        let mut output = surface.data()?;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let offset = y * stride + x * channels;
                let rgb = &data[offset..offset + channels];
                let a = if channels == 4 {
                    u32::from(rgb[3])
                } else {
                    255
                };
                let premul = |v: u8| (u32::from(v) * a + 127) / 255;
                let pixel =
                    (a << 24) | (premul(rgb[0]) << 16) | (premul(rgb[1]) << 8) | premul(rgb[2]);
                let destination = y * output_stride + x * 4;
                output[destination..destination + 4].copy_from_slice(&pixel.to_ne_bytes());
            }
        }
    }
    surface.mark_dirty();
    let square = ImageSurface::create(cairo::Format::ARgb32, pixels, pixels)?;
    let cr = cairo::Context::new(&square)?;
    cr.set_source_surface(
        &surface,
        f64::from(pixels - w) / 2.,
        f64::from(pixels - h) / 2.,
    )?;
    cr.paint()?;
    drop(cr);
    Ok(square)
}

fn read_bounded(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= limit,
        "icon file too large"
    );
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    anyhow::ensure!(data.len() as u64 <= limit, "icon file grew while reading");
    Ok(data)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("niri-spine-icon-{}-{unique}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, path: &str, content: &str) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn theme_lookup_preserves_size_scale_root_order_and_recursive_inheritance() {
        let dir = TempDir::new();
        let index = "[Icon Theme]\nDirectories=16/apps,32/apps\nScaledDirectories=16@2/apps\nInherits=Parent\n[16/apps]\nSize=16\nType=Fixed\n[32/apps]\nSize=32\nType=Fixed\n[16@2/apps]\nSize=16\nScale=2\nType=Fixed\n";
        dir.write("first/Child/index.theme", index);
        dir.write("first/Child/16/apps/test.png", "16");
        dir.write("second/Child/32/apps/test.png", "32");
        dir.write("first/Child/16@2/apps/scaled.png", "scaled");
        dir.write("first/Parent/index.theme", "[Icon Theme]\nDirectories=16/apps\nInherits=Grandparent,Child\n[16/apps]\nSize=16\nType=Fixed\n");
        dir.write(
            "first/Grandparent/index.theme",
            "[Icon Theme]\nDirectories=16/apps\nInherits=Parent\n[16/apps]\nSize=16\nType=Fixed\n",
        );
        dir.write("first/Grandparent/16/apps/inherited.svg", "inherited");
        let roots = vec![dir.0.join("first"), dir.0.join("second")];
        let lookup =
            |name, pixels| lookup_theme(&roots, "Child", name, pixels, &mut HashSet::new());
        assert_eq!(
            lookup("test", 16).unwrap(),
            dir.0.join("first/Child/16/apps/test.png")
        );
        assert_eq!(
            lookup("test", 32).unwrap(),
            dir.0.join("second/Child/32/apps/test.png")
        );
        assert_eq!(
            lookup("test", 20).unwrap(),
            dir.0.join("first/Child/16/apps/test.png")
        );
        assert_eq!(
            lookup("scaled", 32).unwrap(),
            dir.0.join("first/Child/16@2/apps/scaled.png")
        );
        assert_eq!(
            lookup("inherited", 16).unwrap(),
            dir.0.join("first/Grandparent/16/apps/inherited.svg")
        );
        assert!(lookup("missing", 16).is_none());
        assert!(lookup_theme(&roots, "../Child", "test", 16, &mut HashSet::new()).is_none());
        assert!(!relative_directory("../../escape"));
        assert!(!single_name("../org.example.App"));
        assert!(!single_name("org.example.App\0"));
    }

    #[test]
    fn image_decode_preserves_colour_alpha_and_aspect_ratio_without_rotation() {
        let dir = TempDir::new();
        let path = dir.0.join("colour.png");
        let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 4, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[200, 40, 10, 128].repeat(8))
            .unwrap();
        let surface = decode(&path, 16).unwrap();
        assert_eq!((surface.width(), surface.height()), (16, 16));
        surface
            .with_data(|data| {
                let pixel = |x: usize, y: usize| {
                    u32::from_ne_bytes(
                        data[(y * surface.stride() as usize + x * 4)..][..4]
                            .try_into()
                            .unwrap(),
                    )
                };
                assert_eq!(pixel(8, 0), 0);
                assert_eq!(pixel(8, 8), 0x80641405);
            })
            .unwrap();
        dir.write("invalid.png", "not an image");
        assert!(decode(&dir.0.join("invalid.png"), 16).is_err());
        dir.write("vector.svg", "<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'><rect width='16' height='8' fill='#ff0000'/><rect y='8' width='16' height='8' fill='#0000ff'/></svg>");
        let svg = decode(&dir.0.join("vector.svg"), 20).unwrap();
        svg.with_data(|data| {
            let pixel = |x: usize, y: usize| {
                u32::from_ne_bytes(
                    data[(y * svg.stride() as usize + x * 4)..][..4]
                        .try_into()
                        .unwrap(),
                )
            };
            assert_eq!(pixel(10, 2), 0xffff0000);
            assert_eq!(pixel(10, 17), 0xff0000ff);
        })
        .unwrap();
    }

    #[test]
    fn icon_cache_remembers_success_and_failure_until_size_id_or_reload_changes() {
        let mut cache = AppIconCache::default();
        let loads = Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Some(ImageSurface::create(cairo::Format::ARgb32, 16, 16).unwrap())
        };
        assert!(cache.get_with("app", 16, load).is_some());
        assert!(cache
            .get_with("app", 16, || panic!("unchanged icon reloaded"))
            .is_some());
        assert_eq!(loads.get(), 1);
        cache.clear();
        assert!(cache.get_with("app", 16, load).is_some());
        assert!(cache.get_with("app", 20, load).is_some());
        assert!(cache.get_with("other", 20, || None).is_none());
        assert!(cache
            .get_with("other", 20, || panic!("failed icon retried"))
            .is_none());
        assert_eq!(loads.get(), 3);
    }
}
