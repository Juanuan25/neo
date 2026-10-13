//! `GET /service-icons/<store-name>` for files passed to `mkServiceMeta.icon`.

use std::sync::Arc;

use rocket::http::Status;
use rocket::{get, State};

use super::super::icons::{IconSet, ServiceIcon};

#[get("/service-icons/<name>")]
pub fn service_icon(name: &str, icons: &State<Arc<IconSet>>) -> Result<ServiceIcon, Status> {
    icons.read(name).ok_or(Status::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Status as HttpStatus};
    use rocket::local::blocking::Client;
    use rocket::routes;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn route_serves_published_icon_and_rejects_the_rest() {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("neo-icon-route-{}-{}", std::process::id(), n));
        fs::create_dir_all(&root).unwrap();
        let name = format!("{}-icon.svg", "a".repeat(32));
        let unpublished = format!("{}-secret.svg", "b".repeat(32));
        fs::write(root.join(&name), b"<svg>ok</svg>").unwrap();
        fs::write(root.join(&unpublished), b"<svg>no</svg>").unwrap();

        let icons = Arc::new(IconSet::new(root.clone()));
        let src = root.join(&name);
        let src = src.to_str().unwrap();
        assert!(icons.publish(src).is_some());

        let rocket = rocket::build()
            .manage(icons)
            .mount("/", routes![service_icon]);
        let client = Client::tracked(rocket).expect("icon route");

        let ok = client.get(format!("/service-icons/{name}")).dispatch();
        assert_eq!(ok.status(), HttpStatus::Ok);
        assert_eq!(ok.content_type(), Some(ContentType::SVG));
        let headers = ok.headers().clone();
        assert!(headers
            .get_one("Content-Security-Policy")
            .unwrap()
            .contains("default-src 'none'"));
        assert_eq!(
            headers.get_one("Cache-Control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(ok.into_bytes().unwrap(), b"<svg>ok</svg>");

        let hidden = client
            .get(format!("/service-icons/{unpublished}"))
            .dispatch();
        assert_eq!(hidden.status(), HttpStatus::NotFound);

        let junk = client.get("/service-icons/not-a-store-path").dispatch();
        assert_eq!(junk.status(), HttpStatus::NotFound);

        let _ = fs::remove_dir_all::<PathBuf>(root);
    }
}
