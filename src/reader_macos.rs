//! WK resource rules supplement (never replace) the sanitizer and CSP.
//! APIs verified against Wry 0.57.0 and objc2-web-kit 0.3.2 sources.
use anyhow::{Context as _, Result};
use block2::RcBlock;
use futures::channel::oneshot;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
use serde_json::json;
use wry::{WebView, WebViewBuilder, WebViewExtMacOS};

pub(super) const DEFAULT_REMOTE: bool = false;
pub(super) const FIXTURE_URL: &str = "mado://localhost/check-image";

// Only explicit synthetic modes can obtain the offline fixture allowance.
pub(super) fn synthetic() -> bool {
    std::env::args().any(|arg| matches!(arg.as_str(), "--reader-check" | "--reader-demo" | "--demo"))
}

pub(super) fn content_policy(remote: bool, synthetic: bool) -> String {
    let policy = crate::message::content_policy(remote);
    if remote && synthetic { policy.replace("img-src data: https:", "img-src data: https: mado:") }
    else { policy }
}

fn rules(remote: bool, synthetic: bool) -> String {
    // Apple content-blocker syntax: https://developer.apple.com/documentation/safariservices/creating-a-content-blocker
    let mut rules = vec![
        json!({"trigger":{"url-filter":".*"},"action":{"type":"block"}}),
        json!({"trigger":{"url-filter":"^mado://localhost/message([?].*)?$","resource-type":["document"]},"action":{"type":"ignore-previous-rules"}}),
    ];
    for mime in ["png", "jpeg", "gif", "webp", "bmp"] {
        rules.push(json!({"trigger":{"url-filter":format!("^data:image/{mime};base64,"),"resource-type":["image"]},"action":{"type":"ignore-previous-rules"}}));
    }
    if remote {
        rules.push(json!({"trigger":{"url-filter":"^https://[^/@]+/","resource-type":["image"]},"action":{"type":"ignore-previous-rules"}}));
        if synthetic {
            rules.push(json!({"trigger":{"url-filter":"^mado://localhost/check-image$","resource-type":["image"]},"action":{"type":"ignore-previous-rules"}}));
        }
        // Restore the internal-origin prohibition after the image exceptions.
        rules.push(json!({"trigger":{"url-filter":"^https://mado[.]localhost[:/]"},"action":{"type":"block"}}));
        rules.push(json!({"trigger":{"url-filter":".*"},"action":{"type":"block-cookies"}}));
    }
    serde_json::to_string(&rules).unwrap()
}

pub(super) struct Policy {
    blocked: Retained<WKContentRuleList>,
    allowed: Retained<WKContentRuleList>,
    // Keep compilation/cache artifacts isolated from the user's WebKit store.
    _directory: tempfile::TempDir,
}

async fn compile(store: &WKContentRuleListStore, name: &str, rules: &str) -> Result<Retained<WKContentRuleList>> {
    let (send, receive) = oneshot::channel();
    let send = std::cell::RefCell::new(Some(send));
    let completion = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        let result = if !error.is_null() {
            // SAFETY: WebKit supplies valid callback arguments for this invocation.
            Err(anyhow::anyhow!("WK rule compilation failed: {}", unsafe { &*error }))
        } else {
            unsafe { Retained::retain(list) }.context("WK rule compiler returned no policy")
        };
        if let Some(send) = send.borrow_mut().take() { let _ = send.send(result); }
    });
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(name)), Some(&NSString::from_str(rules)), Some(&completion));
    }
    receive.await.context("WK rule compiler disconnected")?
}

impl Policy {
    pub(super) async fn prepare(synthetic: bool) -> Result<Self> {
        let mtm = MainThreadMarker::new().context("WK policy requires the main thread")?;
        let directory = tempfile::tempdir()?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(directory.path().to_str().context("Invalid rule cache path")?));
        let store = unsafe { WKContentRuleListStore::storeWithURL(Some(&url), mtm) }.context("WK rule store unavailable")?;
        let blocked = compile(&store, "mado-blocked", &rules(false, synthetic)).await?;
        let allowed = compile(&store, "mado-allowed", &rules(true, synthetic)).await?;
        Ok(Self { blocked, allowed, _directory: directory })
    }

    pub(super) fn apply(&self, webview: &WebView, remote: bool) {
        // UI thread only; attach before loading any message. No unguarded fallback.
        unsafe {
            let manager = webview.manager();
            manager.removeAllContentRuleLists();
            manager.addContentRuleList(if remote { &self.allowed } else { &self.blocked });
        }
    }
}

pub(super) struct Keyboard(Retained<objc2::runtime::AnyObject>);
impl Drop for Keyboard {
    fn drop(&mut self) { unsafe { objc2_app_kit::NSEvent::removeMonitor(&self.0); } }
}

pub(super) fn keyboard(
    webview: &WebView, cx: gpui::AsyncApp, view: gpui::WeakEntity<super::Reader>, host: gpui::AnyWindowHandle,
) -> Result<Keyboard> {
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags as Flags};
    let native = webview.webview();
    let native_window = webview.ns_window();
    let block = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
        // Local monitors run on the UI thread. Ignore other windows and non-reader responders.
        let input = unsafe { event.as_ref() };
        let mut responder = native_window.firstResponder();
        let mut reader_focused = false;
        while let Some(current) = responder {
            if Retained::as_ptr(&current).cast::<()>() == Retained::as_ptr(&native).cast::<()>() { reader_focused = true; break; }
            responder = unsafe { current.nextResponder() };
        }
        if !reader_focused || input.window(MainThreadMarker::new().unwrap()).as_deref() != Some(&*native_window) { return event.as_ptr(); }
        let flags = input.modifierFlags();
        let alt = flags.contains(Flags::Option);
        let cmd = flags.contains(Flags::Command);
        let shift = flags.contains(Flags::Shift);
        let character = input.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default().to_ascii_lowercase();
        let key = match (character.as_str(), alt, cmd) {
            ("i", true, false) => "alt-i",
            ("r", true, false) => "alt-r",
            ("\u{1b}", false, false) => "escape",
            ("\t" | "\u{19}", false, false) if shift => "shift-tab",
            ("\t", false, false) => "tab",
            ("r", false, true) => "cmd-r",
            ("f", false, true) => "cmd-f",
            ("l", false, true) => "cmd-l",
            ("b", false, true) if shift => "cmd-shift-b",
            ("\r", false, true) => "cmd-enter",
            _ => return event.as_ptr(),
        };
        if !input.isARepeat() {
            let view = view.clone();
            cx.spawn(async move |cx| {
                let _ = host.update(cx, |_, window, cx| {
                    let dispatch = view.update(cx, |reader, cx| {
                        if !reader.visible { return false; }
                        if key == "alt-i" { reader.toggle_images(cx); return false; }
                        reader.body_focus.focus(window, cx);
                        true
                    }).unwrap_or(false);
                    // Dispatch can render synchronously: never hold the Reader entity borrow here.
                    if dispatch {
                        if key == "tab" { window.focus_next(cx); }
                        else if key == "shift-tab" { window.focus_prev(cx); }
                        else { window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx); }
                        // Dispatch may render and refocus the body; return native focus afterwards.
                        let _ = view.update(cx, |reader, _| reader.focus_host());
                    }
                });
            }).detach();
        }
        std::ptr::null_mut()
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
        .context("Could not install reader keyboard routing")?;
    Ok(Keyboard(monitor))
}

pub(super) fn restrict(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder.with_navigation_handler(|uri| crate::message::reader_document(&uri))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_never_allows_fixture_or_non_image_network_resources() {
        assert!(!DEFAULT_REMOTE);
        for remote in [false, true] {
            let policy = content_policy(remote, false);
            assert!(!policy.contains("mado:"));
            let live = rules(remote, false);
            assert!(!live.contains("check-image"));
            let rules: serde_json::Value = serde_json::from_str(&live).unwrap();
            assert_eq!(rules[0]["action"]["type"], "block");
            for rule in rules.as_array().unwrap().iter().skip(1) {
                if rule["action"]["type"] == "ignore-previous-rules" {
                    let kind = rule["trigger"]["resource-type"][0].as_str().unwrap();
                    assert!(matches!(kind, "document" | "image"));
                    if rule["trigger"]["url-filter"].as_str().unwrap().starts_with("^https:") {
                        assert!(remote);
                        assert_eq!(kind, "image");
                        assert!(rule["trigger"]["url-filter"].as_str().unwrap().contains("[^/@]+"));
                    }
                }
            }
        }
        assert!(!content_policy(false, true).contains("mado:"));
        assert!(content_policy(true, true).contains("mado:"));
        assert!(rules(true, true).contains("check-image"));
        assert!(!rules(false, true).contains("check-image"));
    }
}
