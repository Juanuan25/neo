//! Service icons declared as files in `mkServiceMeta.icon`.
//!
//! A URL string is left alone. A Nix path survives eval as a store path
//! (`builtins.toJSON` copies the file to `/nix/store/<hash>-<name>`). This
//! module publishes that path at `/service-icons/<hash>-<name>` and serves
//! only paths that an extract actually published. Store paths are immutable,
//! so the URL is safe to cache.

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rocket::http::{ContentType, Header};
use rocket::request::Request;
use rocket::response::{Responder, Response};

/// Icons larger than this are not served. Logos are small; this blocks a
/// mistaken directory dump or a huge asset named like an icon.
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;

/// Nix base32 alphabet (no e, o, u, t).
const NIX_BASE32: &str = "0123456789abcdfghijklmnpqrsvwxyz";

pub struct IconSet {
    root: PathBuf,
    allowed: Mutex<HashSet<String>>,
}

impl IconSet {
    pub fn nix_store() -> Self {
        Self::new(PathBuf::from("/nix/store"))
    }

    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            allowed: Mutex::new(HashSet::new()),
        }
    }

    /// When `icon` is an image file in this store, remember it and return the
    /// URL the UI should use. Anything else (https URL, `/static/…`, missing
    /// file) returns `None` so the caller keeps the original string.
    pub fn publish(&self, icon: &str) -> Option<String> {
        let name = self.store_image_name(icon)?;
        self.resolve_file(&name)?;
        self.allowed.lock().ok()?.insert(name.clone());
        Some(format!("/service-icons/{name}"))
    }

    /// Rewrite a meta icon in place when it is a published store file.
    pub fn link(&self, icon: &mut Option<String>) {
        let Some(raw) = icon.as_deref() else {
            return;
        };
        let Some(url) = self.publish(raw) else {
            return;
        };
        *icon = Some(url);
    }

    /// Bytes for a previously published icon. Unpublished names 404 even if
    /// the file exists in the store.
    pub fn read(&self, name: &str) -> Option<ServiceIcon> {
        if !self.allowed.lock().ok()?.contains(name) {
            return None;
        }
        let path = self.resolve_file(name)?;
        let body = fs::read(&path).ok()?;
        if body.is_empty() || body.len() as u64 > MAX_ICON_BYTES {
            return None;
        }
        let content_type = content_type_for(name)?;
        Some(ServiceIcon { content_type, body })
    }

    fn store_image_name(&self, icon: &str) -> Option<String> {
        // `//host/…` is a protocol-relative URL. Path would otherwise collapse
        // it toward the store root.
        if !icon.starts_with('/') || icon.starts_with("//") || icon.contains('\0') {
            return None;
        }
        let path = Path::new(icon);
        if path.parent()? != self.root.as_path() {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        if !is_store_image_name(name) {
            return None;
        }
        Some(name.to_string())
    }

    /// Direct child of the store root, canonicalized so a symlink cannot
    /// escape the store. Nix store objects are either the file itself or a
    /// symlink to another store path.
    fn resolve_file(&self, name: &str) -> Option<PathBuf> {
        if !is_store_image_name(name) {
            return None;
        }
        let path = self.root.join(name);
        if path.file_name().and_then(|n| n.to_str()) != Some(name) {
            return None;
        }
        let root = fs::canonicalize(&self.root).ok()?;
        let canon = fs::canonicalize(&path).ok()?;
        if !canon.starts_with(&root) || !canon.is_file() {
            return None;
        }
        let len = fs::metadata(&canon).ok()?.len();
        if len == 0 || len > MAX_ICON_BYTES {
            return None;
        }
        Some(canon)
    }
}

pub struct ServiceIcon {
    content_type: ContentType,
    body: Vec<u8>,
}

impl<'r> Responder<'r, 'static> for ServiceIcon {
    fn respond_to(self, _: &'r Request<'_>) -> rocket::response::Result<'static> {
        Response::build()
            .header(self.content_type)
            .header(Header::new("X-Content-Type-Options", "nosniff"))
            // Direct navigation to an SVG must not run script in the neo origin.
            .header(Header::new(
                "Content-Security-Policy",
                "default-src 'none'; style-src 'unsafe-inline'; sandbox",
            ))
            .header(Header::new(
                "Cache-Control",
                "public, max-age=31536000, immutable",
            ))
            .sized_body(self.body.len(), Cursor::new(self.body))
            .ok()
    }
}

fn is_nix_hash(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| NIX_BASE32.contains(c))
}

fn is_store_image_name(name: &str) -> bool {
    if name.len() > 255 {
        return false;
    }
    let Some((hash, rest)) = name.split_once('-') else {
        return false;
    };
    if rest.is_empty() || rest.contains('/') || rest.contains('\\') {
        return false;
    }
    is_nix_hash(hash) && content_type_for(name).is_some()
}

fn content_type_for(name: &str) -> Option<ContentType> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "svg" => ContentType::SVG,
        "png" => ContentType::PNG,
        "jpg" | "jpeg" => ContentType::JPEG,
        "gif" => ContentType::GIF,
        "webp" => ContentType::WEBP,
        "ico" => ContentType::Icon,
        "avif" => ContentType::new("image", "avif"),
        "bmp" => ContentType::new("image", "bmp"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempStore(PathBuf);

    impl TempStore {
        fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "neo-icons-{}-{}-{}",
                std::process::id(),
                nanos,
                n
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn hash_name(suffix: &str) -> String {
        format!("{}-{suffix}", "a".repeat(32))
    }

    fn write_icon(store: &TempStore, name: &str, bytes: &[u8]) -> String {
        let path = store.path().join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn url_strings_stay_urls() {
        let store = TempStore::new();
        let icons = IconSet::new(store.path().to_path_buf());
        assert!(icons
            .publish("https://cdn.example/icons/gitea.svg")
            .is_none());
        assert!(icons.publish("http://example.com/a.png").is_none());
        assert!(icons.publish("/static/neo-icon.png").is_none());
        assert!(icons
            .publish("//cdn.example/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-icon.svg")
            .is_none());

        let mut icon = Some("https://example.com/a.svg".to_string());
        icons.link(&mut icon);
        assert_eq!(icon.as_deref(), Some("https://example.com/a.svg"));

        let mut site = Some("/static/neo-icon.png".to_string());
        icons.link(&mut site);
        assert_eq!(site.as_deref(), Some("/static/neo-icon.png"));
    }

    #[test]
    fn store_file_is_linked_and_readable_only_after_publish() {
        let store = TempStore::new();
        let name = hash_name("icon.svg");
        let src = write_icon(
            &store,
            &name,
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        );
        // Same shape, different file, never published.
        let other = format!("{}-other.png", "b".repeat(32));
        write_icon(&store, &other, b"\x89PNG\r\n");

        let icons = IconSet::new(store.path().to_path_buf());
        assert!(
            icons.read(&name).is_none(),
            "unpublished store file must not be served"
        );

        let mut icon = Some(src);
        icons.link(&mut icon);
        assert_eq!(
            icon.as_deref(),
            Some(format!("/service-icons/{name}").as_str())
        );

        let served = icons.read(&name).expect("published icon");
        assert_eq!(served.body, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        assert_eq!(served.content_type, ContentType::SVG);
        assert!(icons.read(&other).is_none());
    }

    #[test]
    fn rejects_non_images_escapes_and_huge_files() {
        let store = TempStore::new();
        let icons = IconSet::new(store.path().to_path_buf());

        let text = hash_name("notes.txt");
        let text_src = write_icon(&store, &text, b"hello");
        assert!(icons.publish(&text_src).is_none());

        let bad_hash = format!("{}-icon.svg", "e".repeat(32));
        let bad_src = write_icon(&store, &bad_hash, b"<svg/>");
        assert!(icons.publish(&bad_src).is_none());

        let nested = store.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        let nested_file = nested.join(hash_name("icon.svg"));
        fs::write(&nested_file, b"<svg/>").unwrap();
        assert!(icons.publish(nested_file.to_str().unwrap()).is_none());

        let huge_name = format!("{}-big.png", "c".repeat(32));
        let huge_src = write_icon(
            &store,
            &huge_name,
            &vec![0u8; (MAX_ICON_BYTES as usize) + 1],
        );
        assert!(icons.publish(&huge_src).is_none());

        let empty_name = format!("{}-empty.svg", "d".repeat(32));
        let empty_src = write_icon(&store, &empty_name, b"");
        assert!(icons.publish(&empty_src).is_none());

        let outside = std::env::temp_dir().join(format!(
            "neo-icons-outside-{}-{}.svg",
            std::process::id(),
            "f".repeat(8)
        ));
        fs::write(&outside, b"<svg/>secret</svg>").unwrap();
        let link_name = format!("{}-link.svg", "f".repeat(32));
        symlink(&outside, store.path().join(&link_name)).unwrap();
        let link_src = store.path().join(&link_name);
        assert!(
            icons.publish(link_src.to_str().unwrap()).is_none(),
            "symlink out of the store must not be served"
        );
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn symlink_inside_the_store_is_served() {
        let store = TempStore::new();
        let real = format!("{}-real.png", "a".repeat(32));
        write_icon(&store, &real, b"\x89PNG\r\n\x1a\n");
        let link_name = format!("{}-link.png", "b".repeat(32));
        symlink(store.path().join(&real), store.path().join(&link_name)).unwrap();
        let icons = IconSet::new(store.path().to_path_buf());
        let src = store.path().join(&link_name).to_str().unwrap().to_string();
        let url = icons.publish(&src).expect("in-store symlink");
        assert_eq!(url, format!("/service-icons/{link_name}"));
        let served = icons.read(&link_name).unwrap();
        assert_eq!(served.body, b"\x89PNG\r\n\x1a\n");
        assert_eq!(served.content_type, ContentType::PNG);
    }
}
