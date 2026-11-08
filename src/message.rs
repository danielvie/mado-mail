use ammonia::{Builder, UrlRelative};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use oauth2::url::Url;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
pub const READER_URL: &str = "https://mado.localhost/message";
#[cfg(target_os = "macos")]
pub const READER_URL: &str = "mado://localhost/message";

pub struct MessageBody {
    html: String,
    images: HashMap<String, String>,
    pub warnings: Vec<String>,
}

pub fn raster_image(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp"
    )
}

pub fn external_link(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http" | "mailto")
            && url.host_str() != Some("mado.localhost")
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub fn remote_image(uri: &str, allowed: bool) -> bool {
    allowed
        && Url::parse(uri).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str() != Some("mado.localhost")
                && url.username().is_empty()
                && url.password().is_none()
        })
}

pub fn reader_document(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|url| {
        ((cfg!(target_os = "windows") && url.scheme() == "https" && url.host_str() == Some("mado.localhost"))
            || (cfg!(target_os = "macos") && url.scheme() == "mado" && url.host_str() == Some("localhost")))
            && url.port().is_none()
            && url.path() == "/message"
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub fn content_policy(remote: bool) -> String {
    format!(
        "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:{}; \
        font-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; \
        media-src 'none'; base-uri 'none'; form-action 'none'",
        if remote { " https:" } else { "" }
    )
}

impl MessageBody {
    pub fn html(html: String) -> Self {
        Self {
            html,
            images: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    pub fn plain(text: &str) -> Self {
        Self::html(format!(
            "<pre style=\"white-space:pre-wrap;overflow-wrap:anywhere;font:16px/1.5 'Segoe UI',sans-serif\">{}</pre>",
            ammonia::clean_text(text)
        ))
    }

    pub fn add_image(&mut self, cid: &str, mime: &str, bytes: &[u8]) {
        let mime = mime.to_ascii_lowercase();
        let valid = match mime.as_str() {
            "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
            "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
            "image/bmp" => bytes.starts_with(b"BM"),
            _ => false,
        };
        if valid {
            self.images.insert(
                cid.into(),
                format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
            );
        } else {
            self.warnings
                .push("An embedded image has an unsupported or invalid format.".into());
        }
    }

    pub fn document(&self) -> String {
        let images = self.images.clone();
        let mut cleaner = Builder::default();
        // Email CSS stays inside the child window. CSP and the native request guard block CSS network loads.
        cleaner
            .add_tags(&["style", "font", "center"])
            .rm_clean_content_tags(&["style"])
            .add_generic_attributes(&[
                "style", "class", "id", "align", "bgcolor", "width", "height",
            ])
            .add_tag_attributes("table", &["cellpadding", "cellspacing", "border"])
            .add_tag_attributes("font", &["color", "face", "size"])
            .url_schemes(["https", "http", "mailto", "cid", "data"].into())
            .url_relative(UrlRelative::Deny)
            .set_tag_attribute_value("a", "target", "_self")
            .attribute_filter(move |tag, attribute, value| {
                if tag == "img" && attribute == "src" {
                    let value = value.trim();
                    if let Some((scheme, cid)) = value.split_once(':')
                        && scheme.eq_ignore_ascii_case("cid")
                    {
                        let cid = percent_encoding::percent_decode_str(cid)
                            .decode_utf8()
                            .ok()?;
                        return images.get(cid.as_ref()).cloned().map(Into::into);
                    }
                    if remote_image(value, true)
                        || ["png", "jpeg", "gif", "webp", "bmp"]
                            .iter()
                            .any(|mime| value.starts_with(&format!("data:image/{mime};base64,")))
                    {
                        return Some(value.to_string().into());
                    }
                    return None;
                }
                if tag == "a" && attribute == "href" && !external_link(value) {
                    return None;
                }
                Some(value.to_string().into())
            });
        let html = cleaner.clean(&self.html).to_string();
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\">\
            <style>html{{color-scheme:light}}body{{margin:16px;background:white;color:#202020;font:16px/1.5 'Segoe UI',sans-serif}}\
            img{{max-width:100%}}pre{{white-space:pre-wrap}}a{{overflow-wrap:anywhere}}</style></head><body>{html}</body></html>"
        )
    }
}

pub fn demo() -> MessageBody {
    let mut body = MessageBody::html(include_str!("../fixtures/reader-demo.html").into());
    body.add_image(
        "banner",
        "image/png",
        include_bytes!("../fixtures/reader-banner.png"),
    );
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_active_content_but_preserves_email_layout() {
        let body = MessageBody::html(r#"<style>.card{color:blue;background:url(https://example.invalid/css.png)}</style>
            <table style="background:#fff" cellpadding="12"><tr><td>Hello</td></tr></table>
            <script>alert(1)</script><iframe src="https://example.invalid"></iframe>
            <object data="file:///secret"></object><form action="https://example.invalid"><input></form>
            <meta http-equiv="refresh" content="0;url=https://example.invalid"><base href="file:///">
            <a href="https://example.com" target="_blank" onclick="alert(1)">Safe link</a>
            <a href="javascript:alert(1)">Unsafe</a><a href="file:///secret">File</a>
            <img src="file:///secret" onerror="alert(1)"><img src="data:image/svg+xml,evil">
            <img src="https://example.invalid/pixel" srcset="https://example.invalid/other 2x">"#.into());
        let doc = body.document();
        for forbidden in [
            "<script",
            "<iframe",
            "<object",
            "<form",
            "<input",
            "http-equiv",
            "<base",
            "onclick",
            "onerror",
            "javascript:",
            "file:///",
            "image/svg",
            "srcset",
            "_blank",
        ] {
            assert!(!doc.contains(forbidden), "Allowed {forbidden}");
        }
        assert!(doc.contains("<table style="));
        assert!(doc.contains("cellpadding=\"12\""));
        assert!(doc.contains("<style>.card"));
        assert!(doc.contains("target=\"_self\""));
        assert!(doc.contains("https://example.invalid/pixel"));
        assert!(!content_policy(false).contains("https:"));
        assert!(content_policy(true).contains("img-src data: https:"));
    }

    #[test]
    fn embedded_images_resolve_without_remote_permission_and_text_is_escaped() {
        let mut body =
            MessageBody::html("<img src=\"CID:logo%40mail\"><img src=\"cid:missing\">".into());
        body.add_image(
            "logo@mail",
            "image/png",
            include_bytes!("../fixtures/reader-banner.png"),
        );
        let doc = body.document();
        assert!(doc.contains("data:image/png;base64,"));
        assert!(!doc.contains("cid:"));
        let text = MessageBody::plain("<script>& hello\nnext").document();
        assert!(text.contains("&lt;script&gt;&amp; hello\nnext"));
        body.add_image("evil", "image/png", b"<svg onload=evil>");
        assert_eq!(body.warnings.len(), 1);
    }

    #[test]
    fn request_and_link_policy_rejects_non_images_and_privileged_urls() {
        for uri in [
            "file:///secret",
            "javascript:evil()",
            "data:text/html,evil",
            "https://mado.localhost/secret",
            "https://user:password@example.com",
            "mado://message",
        ] {
            assert!(!external_link(uri));
            assert!(!remote_image(uri, true));
        }
        assert!(!remote_image("https://example.com/image", false));
        assert!(!remote_image("http://example.com/image", true));
        assert!(remote_image("https://example.com/image", true));
        assert!(external_link("mailto:person@example.com"));
        assert!(reader_document(READER_URL));
        assert!(reader_document(&format!("{READER_URL}?revision=2")));
        assert!(!reader_document("https://mado.localhost.evil/message"));
        assert!(!reader_document("https://mado.localhost:8443/message"));
        assert!(!reader_document("mado://localhost/other"));
        assert!(!reader_document("mado://user@localhost/message"));
        assert!(!reader_document("mado://localhost:8443/message"));
        assert!(!reader_document("mado://localhost.evil/message"));
        #[cfg(target_os = "macos")]
        assert!(!reader_document("https://mado.localhost/message"));
    }
}
