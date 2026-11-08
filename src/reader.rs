use crate::{
    gmail::{Email, SharedSession},
    message::{self, MessageBody, READER_URL},
    theme::Palette,
};
use anyhow::{Context as _, Result};
#[cfg(target_os = "windows")]
use gpui::{AnyWindowHandle, AsyncApp, WeakEntity};
use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, Render, TitlebarOptions,
    Window, WindowBounds, WindowHandle, WindowOptions, actions, canvas, div, prelude::*, px, size,
};
use std::{
    borrow::Cow,
    rc::Rc,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};
#[cfg(target_os = "windows")]
use webview2_com::{
    AcceleratorKeyPressedEventHandler, FocusChangedEventHandler, MoveFocusRequestedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT, COREWEBVIEW2_PERMISSION_STATE_DENY,
        COREWEBVIEW2_PHYSICAL_KEY_STATUS, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
    },
    NavigationStartingEventHandler, PermissionRequestedEventHandler,
    WebResourceRequestedEventHandler,
};
#[cfg(target_os = "windows")]
use windows::core::{HSTRING, PWSTR};
#[cfg(target_os = "windows")]
use wry::{WebViewBuilderExtWindows, WebViewExtWindows};
use wry::{
    WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
    http::Response,
};

actions!(mail_reader, [ToggleImages]);

#[cfg(target_os = "windows")]
#[path = "detach_check.rs"]
pub(crate) mod detach_check;
#[cfg(target_os = "macos")]
#[path = "reader_macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
#[path = "macos_reader_check.rs"]
mod macos_reader_check;
#[cfg(target_os = "windows")]
const DEFAULT_REMOTE: bool = true;
#[cfg(target_os = "macos")]
const DEFAULT_REMOTE: bool = macos::DEFAULT_REMOTE;

#[derive(Clone)]
struct Document {
    html: String,
    remote: bool,
}

pub struct Reader {
    focus: FocusHandle,
    body_focus: FocusHandle,
    detach_focus: FocusHandle,
    detachable: bool,
    email: Email,
    labels: Vec<crate::labels::Badge>,
    loaded: bool,
    document: Arc<RwLock<Document>>,
    generation: Arc<AtomicU64>,
    webview: Option<Rc<WebView>>,
    #[cfg(target_os = "macos")]
    policy: Option<macos::Policy>,
    #[cfg(target_os = "macos")]
    keyboard: Option<macos::Keyboard>,
    webview_error: Option<String>,
    status: String,
    failed: bool,
    visible: bool,
    page_ready: bool,
}

// WebView2 callbacks run on the UI thread. Schedule browser opening rather than re-entering GPUI.
#[cfg(target_os = "windows")]
fn restrict(
    webview: &WebView,
    document: Arc<RwLock<Document>>,
    cx: AsyncApp,
    view: WeakEntity<Reader>,
    host: AnyWindowHandle,
) -> Result<()> {
    unsafe {
        let focus_view = view.clone();
        let focus_cx = cx.clone();
        webview.controller().add_GotFocus(&FocusChangedEventHandler::create(Box::new(move |_, _| {
            let view = focus_view.clone();
            focus_cx.spawn(async move |cx| {
                let _ = host.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        if this.visible && windows::Win32::UI::Input::KeyboardAndMouse::GetFocus()
                            != windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow()
                        { this.body_focus.focus(window, cx); }
                    });
                });
            }).detach();
            Ok(())
        })), &mut 0)?;
        let focus_view = view.clone();
        let focus_cx = cx.clone();
        webview.controller().add_MoveFocusRequested(&MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()); };
            let mut reason = COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT;
            args.Reason(&mut reason)?;
            args.SetHandled(true)?;
            let view = focus_view.clone();
            focus_cx.spawn(async move |cx| {
                let _ = host.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |this, _| {
                        if let Some(webview) = &this.webview { let _ = webview.focus_parent(); }
                    });
                    if reason == COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT { window.focus_next(cx); }
                    else { window.focus_prev(cx); }
                });
            }).detach();
            Ok(())
        })), &mut 0)?;
        let keyboard_cx = cx.clone();
        webview.controller().add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut key = 0;
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
                let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                args.VirtualKey(&mut key)?;
                args.KeyEventKind(&mut kind)?;
                args.PhysicalKeyStatus(&mut status)?;
                if key == 0x49
                    && kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    && status.IsMenuKeyDown.as_bool()
                {
                    args.SetHandled(true)?;
                    if !status.WasKeyDown.as_bool() {
                        let view = view.clone();
                        keyboard_cx
                            .spawn(async move |cx| {
                                let _ = view.update(cx, |this, cx| this.toggle_images(cx));
                            })
                            .detach();
                    }
                }
                if (key == 0x1B && kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN)
                    || (key == 0x52 && kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN && status.IsMenuKeyDown.as_bool())
                {
                    args.SetHandled(true)?;
                    let view = view.clone();
                    keyboard_cx.spawn(async move |cx| {
                        let _ = host.update(cx, |_, window, cx| {
                            let _ = view.update(cx, |this, _| {
                                if let Some(webview) = &this.webview { let _ = webview.focus_parent(); }
                            });
                            window.dispatch_keystroke(gpui::Keystroke::parse(if key == 0x1B { "escape" } else { "alt-r" }).unwrap(), cx);
                        });
                    }).detach();
                }
                Ok(())
            })),
            &mut 0,
        )?;
        let core = webview.webview();
        let settings = core.Settings()?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                }
                Ok(())
            })),
            &mut 0,
        )?;
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = webview2_com::take_pwstr(uri);
                if message::reader_document(&uri) {
                    return Ok(());
                }
                args.SetCancel(true)?;
                let mut user = false.into();
                args.IsUserInitiated(&mut user)?;
                if user.as_bool() && message::external_link(&uri) {
                    cx.spawn(async move |cx| {
                        cx.update(|cx| cx.open_url(&uri));
                    })
                    .detach();
                }
                Ok(())
            })),
            &mut 0,
        )?;
        core.AddWebResourceRequestedFilter(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )?;
        let environment = webview.environment();
        let synthetic_check = std::env::args().any(|arg| {
            matches!(
                arg.as_str(),
                "--reader-check" | "--reader-demo" | "--demo" | "--triage-check"
            )
        });
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let request = args.Request()?;
                let mut uri = PWSTR::null();
                request.Uri(&mut uri)?;
                let uri = webview2_com::take_pwstr(uri);
                let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
                args.ResourceContext(&mut context)?;
                let allowed = (context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT
                    && message::reader_document(&uri))
                    || (context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE
                        && message::remote_image(&uri, document.read().unwrap().remote));
                if synthetic_check
                    && allowed
                    && context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE
                    && uri == "https://example.invalid/remote-banner.png"
                {
                    // Offline browser check substitutes only the synthetic fixture, never live Gmail content.
                    let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(
                        include_bytes!("../fixtures/reader-banner.png"),
                    ))
                    .unwrap();
                    let response = environment.CreateWebResourceResponse(
                        &stream,
                        200,
                        &HSTRING::from("OK"),
                        &HSTRING::from("Content-Type: image/png\r\nCache-Control: no-store\r\n"),
                    )?;
                    args.SetResponse(&response)?;
                } else if !allowed
                    || (synthetic_check && context != COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT)
                {
                    let response = environment.CreateWebResourceResponse(
                        None,
                        403,
                        &HSTRING::from("Blocked"),
                        &HSTRING::from("Content-Type: text/plain\r\n"),
                    )?;
                    args.SetResponse(&response)?;
                }
                Ok(())
            })),
            &mut 0,
        )?;
    }
    Ok(())
}

fn native_bounds(bounds: Bounds<gpui::Pixels>, scale: f64) -> wry::Rect {
    let position = LogicalPosition::new(f64::from(f32::from(bounds.origin.x)), f64::from(f32::from(bounds.origin.y)));
    let size = LogicalSize::new(f64::from(f32::from(bounds.size.width)), f64::from(f32::from(bounds.size.height)));
    #[cfg(target_os = "windows")]
    { wry::Rect { position: position.to_physical::<i32>(scale).into(), size: size.to_physical::<i32>(scale).into() } }
    #[cfg(target_os = "macos")]
    {
        // WK bounds() uses logical points, and set_bounds() rounds sizes to integral points.
        let _ = scale;
        wry::Rect { position: position.into(), size: LogicalSize::new(size.width.round(), size.height.round()).into() }
    }
}

#[cfg(all(test, target_os = "macos"))]
#[test]
fn retina_reader_bounds_remain_in_logical_points() {
    let bounds = Bounds::new(gpui::point(px(100.5), px(200.)), size(px(480.), px(329.)));
    let rect = native_bounds(bounds, 2.);
    assert_eq!(rect.position, wry::dpi::Position::Logical(LogicalPosition::new(100.5, 200.)));
    assert_eq!(rect.size, wry::dpi::Size::Logical(LogicalSize::new(480., 329.)));
    assert_eq!(rect, native_bounds(bounds, 1.));
}

impl Reader {
    pub fn open(cx: &mut App, title: impl Into<gpui::SharedString>) -> Result<WindowHandle<Self>> {
        let title = title.into();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(960.), px(800.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(title),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut reader = Self::new(window, cx);
                    reader.detachable = false;
                    reader.focus.focus(window, cx);
                    reader
                })
            },
        )
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.defer_in(window, |this: &mut Self, window, cx| this.mount(window, cx));
        let body_focus = cx.focus_handle();
        cx.on_focus(&body_focus, window, |this: &mut Self, window, _| {
            // A frame's focus event must not override a newer host focus change.
            if this.visible && this.body_focus.is_focused(window)
                && let Some(webview) = &this.webview { let _ = webview.focus(); }
        }).detach();
        Self {
            focus: cx.focus_handle(),
            body_focus,
            detach_focus: cx.focus_handle(),
            detachable: true,
            email: Email {
                id: String::new(), from: String::new(), subject: "Message reader".into(),
                date: String::new(), snippet: String::new(), label_ids: Vec::new(),
            },
            labels: Vec::new(),
            loaded: false,
            document: Arc::new(RwLock::new(Document {
                html: MessageBody::plain("Select a message.").document(), remote: DEFAULT_REMOTE,
            })),
            generation: Arc::new(AtomicU64::new(0)),
            webview: None,
            #[cfg(target_os = "macos")]
            policy: None,
            #[cfg(target_os = "macos")]
            keyboard: None,
            webview_error: None,
            status: "Select a message. Preview preserves unread status.".into(),
            failed: false,
            visible: true,
            page_ready: false,
        }
    }

    fn can_detach(&self) -> bool {
        self.detachable && self.visible && self.loaded && !self.failed
    }

    fn detach(&mut self, cx: &mut Context<Self>) {
        if !self.can_detach() { return; }
        let result = (|| -> Result<()> {
            let window = Self::open(cx, format!("{} - Mado Mail", self.email.subject))?;
            let email = self.email.clone();
            let labels = self.labels.clone();
            let document = self.document.read().unwrap().clone();
            let status = self.status.clone();
            window.update(cx, |reader, window, cx| {
                reader.email = email;
                reader.labels = labels;
                *reader.document.write().unwrap() = document;
                reader.status = status;
                reader.loaded = true;
                window.activate_window();
                cx.notify();
            })?;
            Ok(())
        })();
        if let Err(error) = result {
            self.status = format!("Could not open a separate reader window: {error:#}");
            cx.notify();
        }
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            if !visible && let Some(webview) = &self.webview {
                let _ = webview.focus_parent();
                let _ = webview.set_visible(false);
            }
            // Showing waits for layout to update the native child's bounds.
            cx.notify();
        }
    }

    pub fn focus_host(&self) {
        if let Some(webview) = &self.webview { let _ = webview.focus_parent(); }
    }

    pub fn set_labels(&mut self, labels: Vec<crate::labels::Badge>, cx: &mut Context<Self>) {
        if self.labels != labels { self.labels = labels; cx.notify(); }
    }

    pub fn needs_load(&self, id: &str) -> bool {
        self.email.id != id || self.failed
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn browser(&self) -> Option<Rc<WebView>> {
        self.webview.clone()
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use wry::raw_window_handle::HasWindowHandle;
        let raw = match HasWindowHandle::window_handle(window) {
            Ok(handle) => handle.as_raw(),
            Err(error) => {
                self.webview_error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let host = Window::window_handle(window);
        let document = self.document.clone();
        let generation = self.generation.clone();
        // Wry pumps Win32 messages during creation. Never hold a GPUI App/Entity borrow across it.
        cx.spawn(async move |this, cx| {
        if host.update(cx, |_, _, _| ()).is_err() { return; }
        #[cfg(target_os = "macos")]
        let policy = macos::Policy::prepare(macos::synthetic()).await;
        if host.update(cx, |_, _, _| ()).is_err() { return; }
        let result = (|| {
            #[cfg(target_os = "macos")]
            let policy = policy.context("WKWebView resource protection could not start; reader disabled")?;
            // SAFETY: this UI-thread task uses the host's native handle only for child creation.
            // A closed host rejects attachment; the result is kept only if the host still exists.
            let parent = unsafe { wry::raw_window_handle::WindowHandle::borrow_raw(raw) };
            let resources = document.clone();
            let view = this.clone();
            let app = cx.clone();
            let builder = WebViewBuilder::new()
                .with_visible(false)
                .with_on_page_load_handler(move |event, uri| {
                    if !matches!(event, wry::PageLoadEvent::Finished) { return; }
                    let view = view.clone();
                    app.spawn(async move |cx| {
                        let _ = view.update(cx, |this, cx| {
                            let current = format!("{READER_URL}?revision={}", this.generation.load(Ordering::Relaxed));
                            if uri == current {
                                this.page_ready = true;
                                cx.notify();
                            }
                        });
                    }).detach();
                })
                .with_javascript_disabled().with_devtools(false).with_incognito(true)
                .with_general_autofill_enabled(false)
                .with_hotkeys_zoom(true).with_autoplay(false)
                .with_drag_drop_handler(|_| true)
                .with_download_started_handler(|_, _| false)
                .with_bounds(wry::Rect { position: LogicalPosition::new(0., 160.).into(), size: LogicalSize::new(960., 640.).into() });
            #[cfg(target_os = "windows")]
            let builder = builder.with_default_context_menus(false).with_browser_accelerator_keys(false).with_https_scheme(true);
            #[cfg(target_os = "macos")]
            let builder = macos::restrict(builder);
            let webview = builder.with_custom_protocol("mado".into(), move |_, request| {
                    let doc = resources.read().unwrap();
                    // The protocol receives mado://localhost/message after Wry reverses its Windows URL mapping.
                    #[cfg(target_os = "macos")]
                    if macos::synthetic() && request.method() == "GET" && request.uri().to_string() == macos::FIXTURE_URL {
                        return Response::builder().header("Content-Type", "image/png").header("Cache-Control", "no-store")
                            .body(Cow::Borrowed(include_bytes!("../fixtures/reader-banner.png").as_slice())).unwrap();
                    }
                    #[cfg(target_os = "windows")]
                    let valid = request.method() == "GET" && request.uri().path() == "/message";
                    #[cfg(target_os = "macos")]
                    let valid = request.method() == "GET" && message::reader_document(&request.uri().to_string());
                    #[cfg(target_os = "windows")]
                    let policy = message::content_policy(doc.remote);
                    #[cfg(target_os = "macos")]
                    let policy = macos::content_policy(doc.remote, macos::synthetic());
                    let html = doc.html.clone();
                    #[cfg(target_os = "macos")]
                    let html = if macos::synthetic() { html.replace("https://example.invalid/remote-banner.png", macos::FIXTURE_URL) } else { html };
                    Response::builder().status(if valid { 200 } else { 404 })
                        .header("Content-Type", "text/html; charset=utf-8")
                        .header("Content-Security-Policy", policy)
                        .header("Referrer-Policy", "no-referrer")
                        .header("Cache-Control", "no-store")
                        .body(Cow::Owned(if valid { html.into_bytes() } else { Vec::new() })).unwrap()
                }).build_as_child(&parent).context("Native message reader could not start (Windows requires the Edge WebView2 Runtime)")?;
            #[cfg(target_os = "windows")]
            restrict(&webview, document.clone(), cx.clone(), this.clone(), host)?;
            #[cfg(target_os = "macos")]
            policy.apply(&webview, document.read().unwrap().remote);
            #[cfg(target_os = "macos")]
            let keyboard = macos::keyboard(&webview, cx.clone(), this.clone(), host)?;
            webview.load_url(&format!("{READER_URL}?revision={}", generation.load(Ordering::Relaxed)))?;
            #[cfg(target_os = "windows")]
            return anyhow::Ok(webview);
            #[cfg(target_os = "macos")]
            anyhow::Ok((webview, policy, keyboard))
        })();
        let _ = host.update(cx, |_, window, cx| {
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(result) => {
                        #[cfg(target_os = "windows")]
                        let webview = result;
                        #[cfg(target_os = "macos")]
                        let webview = { this.policy = Some(result.1); this.keyboard = Some(result.2); result.0 };
                        let webview = Rc::new(webview);
                        #[cfg(target_os = "windows")]
                        if std::env::args().any(|arg| arg == "--reader-check") {
                            crate::reader_check::start(webview.clone(), window, cx);
                        }
                        #[cfg(target_os = "macos")]
                        if std::env::args().any(|arg| arg == "--reader-check") {
                            macos_reader_check::start(webview.clone(), window, cx);
                        }
                        this.webview = Some(webview);
                    }
                    Err(error) => {
                        #[cfg(target_os = "macos")]
                        if std::env::args().any(|arg| arg == "--reader-check") {
                            macos_reader_check::fail(&format!("Reader startup failed: {error:#}"));
                        }
                        this.webview_error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            });
        });
        }).detach();
    }

    fn show(&mut self, body: MessageBody, cx: &mut Context<Self>) {
        self.status = if self.document.read().unwrap().remote {
            "Remote images allowed. They can track opens and IP addresses."
        } else {
            "Remote images blocked. Blocking cannot undo requests already sent."
        }
        .into();
        if !body.warnings.is_empty() {
            self.status.push(' ');
            self.status.push_str(&body.warnings.join(" "));
        }
        self.failed = false;
        self.document.write().unwrap().html = body.document();
        self.loaded = true;
        self.reload(cx);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.page_ready = false;
        if let Some(webview) = &self.webview {
            let _ = webview.set_visible(false);
            #[cfg(target_os = "macos")]
            if let Some(policy) = &self.policy { policy.apply(webview, self.document.read().unwrap().remote); }
            if let Err(error) = webview.load_url(&format!(
                "{READER_URL}?revision={}",
                self.generation.load(Ordering::Relaxed)
            )) {
                self.failed = true;
                self.status = format!("Could not display message: {error}");
            }
        }
        cx.notify();
    }

    pub fn load(&mut self, email: Email, session: SharedSession, cx: &mut Context<Self>) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let current = self.generation.clone();
        self.email = email;
        self.loaded = false;
        self.failed = false;
        self.status = "Loading message...".into();
        *self.document.write().unwrap() = Document {
            html: MessageBody::plain("Loading message...").document(),
            remote: DEFAULT_REMOTE,
        };
        self.reload(cx);
        let id = self.email.id.clone();
        let work = cx.background_executor().spawn(async move {
            let mut session = session
                .lock()
                .map_err(|_| anyhow::anyhow!("Gmail worker failed"))?;
            if current.load(Ordering::Relaxed) != generation {
                return anyhow::Ok(None);
            }
            session.message_body(&id)?.map(Some).context(
                "Google authorization expired or was revoked. Reconnect Gmail in the Inbox",
            )
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.complete(generation, result, cx));
        })
        .detach();
    }

    fn complete(
        &mut self,
        generation: u64,
        result: Result<Option<MessageBody>>,
        cx: &mut Context<Self>,
    ) {
        if self.generation.load(Ordering::Relaxed) != generation {
            return;
        }
        match result {
            Ok(Some(body)) => self.show(body, cx),
            Ok(None) => {}
            Err(error) => {
                self.failed = true;
                self.status = format!("{error:#}. Reopen the message to retry.");
                self.document.write().unwrap().html =
                    MessageBody::plain("Message could not be loaded.").document();
                self.reload(cx);
            }
        }
    }

    pub(crate) fn check_stale_completion(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let old = self.generation.load(Ordering::Relaxed);
        self.demo(cx);
        let html = self.document.read().unwrap().html.clone();
        let status = self.status.clone();
        self.complete(old, Ok(Some(MessageBody::plain("STALE BODY"))), cx);
        self.complete(old, Err(anyhow::anyhow!("STALE ERROR")), cx);
        anyhow::ensure!(
            self.document.read().unwrap().html == html && self.status == status && !self.failed,
            "A stale message completion replaced the current body or status"
        );
        Ok(())
    }

    pub fn demo(&mut self, cx: &mut Context<Self>) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.document.write().unwrap().remote = DEFAULT_REMOTE;
        self.email = Email {
            id: "demo".into(),
            from: "Synthetic club <demo@example.invalid>".into(),
            subject: "Your membership has been renewed".into(),
            date: "Synthetic message, no Gmail access".into(),
            snippet: String::new(),
            label_ids: Vec::new(),
        };
        self.show(message::demo(), cx);
    }

    pub fn demo_message(&mut self, email: Email, cx: &mut Context<Self>) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.document.write().unwrap().remote = DEFAULT_REMOTE;
        self.email = email;
        self.show(message::demo(), cx);
    }

    pub(crate) fn toggle_images(&mut self, cx: &mut Context<Self>) {
        if !self.loaded || self.webview.is_none() {
            return;
        }
        let remote = {
            let mut doc = self.document.write().unwrap();
            doc.remote = !doc.remote;
            doc.remote
        };
        self.status = if remote {
            "Remote images allowed for this message. They can track your open and IP address."
        } else {
            "Remote images blocked. Blocking again cannot undo earlier image requests."
        }
        .into();
        self.reload(cx);
    }
}

impl Render for Reader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let remote = self.document.read().unwrap().remote;
        let webview = self.webview.clone();
        let visible = self.visible && self.page_ready;
        div()
            .key_context("MailReader")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleImages, _, cx| this.toggle_images(cx)))
            .when(!self.detachable, |d| d.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    cx.stop_propagation();
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                } else if event.keystroke.key == "escape" {
                    this.focus_host();
                    this.focus.focus(window, cx);
                    cx.stop_propagation();
                }
            })))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.canvas)
            .text_color(p.ink)
            .font_family(crate::SYSTEM_FONT)
            .text_sm()
            .child(
                div()
                    .p_4()
                    .bg(p.panel)
                    .border_b_1()
                    .border_color(p.line)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_shrink_0()
                    .child(div().flex().items_center().gap_2()
                        .child(div().flex_1().min_w_0().truncate().text_lg().child(self.email.subject.clone()))
                        .when(self.detachable, |d| d.child(div().id("detach-reader")
                            .track_focus(&self.detach_focus.clone().tab_index(0).tab_stop(self.can_detach()))
                            .size(px(28.)).flex_shrink_0().flex().items_center().justify_center().rounded_sm()
                            .border_1().border_color(gpui::transparent_black()).cursor_pointer()
                            .when(!self.can_detach(), |d| d.opacity(0.4))
                            .focus(|s| s.border_color(p.accent)).hover(|s| s.bg(p.panel2))
                            .tooltip(|_, cx| cx.new(|_| crate::Hint("Open this message in a separate window")).into())
                            .on_click(cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.detach(cx); }))
                            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.stop_propagation(); this.detach(cx);
                                }
                            }))
                            .on_key_up(|event, window, _| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") { window.prevent_default(); }
                            })
                            .child(crate::icons::icon("icons/pop-out.svg", p.ink_dim)))))
                    .child(div().truncate().child(self.email.from.clone()))
                    .when(!self.labels.is_empty(), |d| d.child(
                        div().id("reader-labels").max_h(px(96.)).overflow_y_scroll().flex().flex_wrap().gap_1()
                            .children(self.labels.iter().map(|badge| {
                                let (ink, bg) = badge.colors.unwrap_or((p.ink, p.panel2));
                                div().max_w_full().px_1().py(px(1.)).rounded_sm().bg(bg).text_color(ink).text_xs()
                                    .whitespace_normal().child(badge.text.clone())
                            }))
                    ))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(p.ink_dim)
                            .child(self.email.date.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .mt_2()
                            .child(
                                div()
                                    .id("toggle-images")
                                    .flex_shrink_0()
                                    .tab_index(0)
                                    .tab_stop(self.visible)
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(p.accent)
                                    .text_color(p.on_accent)
                                    .border_1()
                                    .border_color(p.accent)
                                    .cursor_pointer()
                                    .when(!self.loaded || self.webview.is_none(), |style| {
                                        style.opacity(0.5)
                                    })
                                    .focus(|style| style.border_color(p.ink))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_images(cx)))
                                    .on_key_down(cx.listener(
                                        |this, event: &gpui::KeyDownEvent, _, cx| {
                                            if event.keystroke.key == "enter"
                                                || event.keystroke.key == "space"
                                            {
                                                cx.stop_propagation();
                                                this.toggle_images(cx);
                                            }
                                        },
                                    ))
                                    .child(if remote {
                                        "Block images"
                                    } else {
                                        "Allow images"
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_xs()
                                    .text_color(if self.failed || self.webview_error.is_some() {
                                        p.danger
                                    } else {
                                        p.ink_dim
                                    })
                                    .child(
                                        self.webview_error.as_ref().unwrap_or(&self.status).clone(),
                                    ),
                            ),
                    ),
            )
            .child(
                div().id("reader-body").tab_index(0).track_focus(&self.body_focus.clone().tab_index(0).tab_stop(self.visible))
                .flex_1().w_full().min_h_0().flex().child(canvas(
                    move |bounds, window, _| {
                        if let Some(webview) = webview {
                            let rect = native_bounds(bounds, f64::from(window.scale_factor()));
                            if webview.bounds().is_ok_and(|old| old != rect) {
                                let _ = webview.set_bounds(rect);
                            }
                            let _ = webview.set_visible(visible && bounds.size.height > px(0.) && bounds.size.width > px(0.));
                        }
                    },
                    |_, _, _, _| {},
                )
                .flex_1()
                .w_full()
                .min_h_0()),
            )
    }
}
