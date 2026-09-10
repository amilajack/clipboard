//! Clipboard questions arboard doesn't answer, asked the way each platform
//! expects: whether a copy was marked as not to be recorded, as password
//! managers do, and when the clipboard changes.
//!
//! When the check itself fails, the copy is taken to be unmarked. Otherwise
//! history would stop working anywhere the check can't run.

pub use imp::{change_count, is_concealed, Changes};

/// Linux and the BSDs, on X11 or Wayland.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
mod imp {
    use std::env;

    use wl_clipboard_rs::paste::{get_mime_types, ClipboardType, Seat};

    /// Introduced by KDE's clipboard manager, and set by password managers
    /// like KeePassXC.
    const HINT: &str = "x-kde-passwordManagerHint";

    /// X11 and Wayland have no cheap change counter; they announce changes
    /// instead.
    pub fn change_count() -> Option<u64> {
        None
    }

    pub fn is_concealed() -> bool {
        // A Wayland program without focus can only see the clipboard through
        // the data-control protocol, which some compositors, like GNOME's,
        // lack. X11 is the fallback, as it is for arboard, and XWayland
        // mirrors the clipboard there.
        if env::var_os("WAYLAND_DISPLAY").is_some() {
            if let Ok(types) = get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
                return types.contains(HINT);
            }
        }
        x11::offers(HINT).unwrap_or(false)
    }

    /// Announcements of clipboard changes. They come from Wayland's
    /// data-control protocol where the compositor has it, since that's where
    /// arboard reads the clipboard too, and otherwise from X11's XFixes
    /// extension, which XWayland also serves.
    pub enum Changes {
        Wayland(wayland::Changes),
        X11(Box<x11::Changes>),
    }

    impl Changes {
        pub fn listen() -> Option<Self> {
            if env::var_os("WAYLAND_DISPLAY").is_some() {
                if let Some(changes) = wayland::Changes::listen() {
                    return Some(Self::Wayland(changes));
                }
            }
            x11::Changes::listen().map(|changes| Self::X11(Box::new(changes)))
        }

        pub fn name(&self) -> &'static str {
            match self {
                Self::Wayland(_) => "Wayland data-control",
                Self::X11(_) => "X11 XFixes",
            }
        }

        /// Blocks until the clipboard changes. Returns false once
        /// announcements stop coming, say because the connection broke.
        pub fn wait(&mut self) -> bool {
            match self {
                Self::Wayland(changes) => changes.wait(),
                Self::X11(changes) => changes.wait(),
            }
        }
    }

    mod x11 {
        use std::thread;
        use std::time::{Duration, Instant};

        use x11rb::connection::Connection;
        use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
        use x11rb::protocol::xproto::{
            AtomEnum, ConnectionExt as _, CreateWindowAux, Window, WindowClass,
        };
        use x11rb::protocol::Event;
        use x11rb::rust_connection::RustConnection;

        /// How long the clipboard's owner gets to list what it offers.
        const TIMEOUT: Duration = Duration::from_millis(500);

        /// Connects, and makes a window to receive selections and events.
        fn connect() -> Option<(RustConnection, Window)> {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let window = conn.generate_id().ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .ok()?;
            Some((conn, window))
        }

        fn atom(conn: &RustConnection, name: &str, only_if_exists: bool) -> Option<u32> {
            let cookie = conn.intern_atom(only_if_exists, name.as_bytes()).ok()?;
            Some(cookie.reply().ok()?.atom)
        }

        /// Whether the clipboard's owner lists `target` among the formats it
        /// offers.
        pub fn offers(target: &str) -> Option<bool> {
            let (conn, window) = connect()?;
            // Nobody can offer a format that no program has named.
            let target = atom(&conn, target, true)?;
            if target == x11rb::NONE {
                return Some(false);
            }
            let clipboard = atom(&conn, "CLIPBOARD", false)?;
            let targets = atom(&conn, "TARGETS", false)?;
            let property = atom(&conn, "CB_TARGETS", false)?;
            conn.convert_selection(window, clipboard, targets, property, x11rb::CURRENT_TIME)
                .ok()?;
            conn.flush().ok()?;

            let deadline = Instant::now() + TIMEOUT;
            loop {
                match conn.poll_for_event().ok()? {
                    Some(Event::SelectionNotify(event)) if event.requestor == window => {
                        // No property means nobody owns the clipboard, or the
                        // owner won't say what it offers.
                        if event.property == x11rb::NONE {
                            return Some(false);
                        }
                        break;
                    }
                    Some(_) => {}
                    None if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                    None => return None,
                }
            }
            let reply = conn
                .get_property(true, window, property, AtomEnum::ATOM, 0, u32::MAX)
                .ok()?
                .reply()
                .ok()?;
            let offered = reply.value32()?.any(|offered| offered == target);
            Some(offered)
        }

        pub struct Changes {
            conn: RustConnection,
        }

        impl Changes {
            pub fn listen() -> Option<Self> {
                let (conn, window) = connect()?;
                // XFixes has to be asked for by version before it's used.
                conn.xfixes_query_version(5, 0).ok()?.reply().ok()?;
                let clipboard = atom(&conn, "CLIPBOARD", false)?;
                // A new owner, or the old one going away, which empties it.
                let events = SelectionEventMask::SET_SELECTION_OWNER
                    | SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | SelectionEventMask::SELECTION_CLIENT_CLOSE;
                conn.xfixes_select_selection_input(window, clipboard, events)
                    .ok()?;
                conn.flush().ok()?;
                Some(Self { conn })
            }

            pub fn wait(&mut self) -> bool {
                loop {
                    match self.conn.wait_for_event() {
                        Ok(Event::XfixesSelectionNotify(_)) => return true,
                        Ok(_) => {}
                        Err(_) => return false,
                    }
                }
            }
        }
    }

    mod wayland {
        use wayland_client::globals::{registry_queue_init, GlobalListContents};
        use wayland_client::protocol::wl_registry::WlRegistry;
        use wayland_client::protocol::wl_seat::WlSeat;
        use wayland_client::{
            event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle,
        };
        use wayland_protocols::ext::data_control::v1::client::{
            ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
            ext_data_control_manager_v1::ExtDataControlManagerV1,
            ext_data_control_offer_v1::ExtDataControlOfferV1,
        };
        use wayland_protocols_wlr::data_control::v1::client::{
            zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
            zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
            zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
        };

        /// What the event handlers tell `wait`.
        #[derive(Default)]
        struct State {
            changed: bool,
            /// The compositor retired our data-control device.
            finished: bool,
        }

        pub struct Changes {
            queue: EventQueue<State>,
            state: State,
            _conn: Connection,
        }

        impl Changes {
            pub fn listen() -> Option<Self> {
                let conn = Connection::connect_to_env().ok()?;
                let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
                let qh = queue.handle();
                let seat: WlSeat = globals.bind(&qh, 1..=2, ()).ok()?;
                // ext-data-control is the standard; wlr-data-control is what
                // came before it, and some compositors still only have that.
                match globals.bind::<ExtDataControlManagerV1, _, _>(&qh, 1..=1, ()) {
                    Ok(manager) => {
                        manager.get_data_device(&seat, &qh, ());
                    }
                    Err(_) => {
                        let manager: ZwlrDataControlManagerV1 =
                            globals.bind(&qh, 1..=2, ()).ok()?;
                        manager.get_data_device(&seat, &qh, ());
                    }
                }
                let mut state = State::default();
                // Have the compositor take all that in before relying on it.
                queue.roundtrip(&mut state).ok()?;
                if state.finished {
                    return None;
                }
                Some(Self {
                    queue,
                    state,
                    _conn: conn,
                })
            }

            pub fn wait(&mut self) -> bool {
                self.state.changed = false;
                while !self.state.changed {
                    if self.state.finished || self.queue.blocking_dispatch(&mut self.state).is_err()
                    {
                        return false;
                    }
                }
                true
            }
        }

        /// Handlers for objects whose events don't matter here.
        macro_rules! ignore_events {
            ($($interface:ty => $data:ty),* $(,)?) => {$(
                impl Dispatch<$interface, $data> for State {
                    fn event(
                        _: &mut Self,
                        _: &$interface,
                        _: <$interface as Proxy>::Event,
                        _: &$data,
                        _: &Connection,
                        _: &QueueHandle<Self>,
                    ) {
                    }
                }
            )*};
        }

        ignore_events!(
            WlRegistry => GlobalListContents,
            WlSeat => (),
            ExtDataControlManagerV1 => (),
            ZwlrDataControlManagerV1 => (),
            ExtDataControlOfferV1 => (),
            ZwlrDataControlOfferV1 => (),
        );

        /// The two protocols' devices differ only in name.
        macro_rules! handle_device {
            ($device:ty, $module:ident, $offer:ty) => {
                impl Dispatch<$device, ()> for State {
                    fn event(
                        state: &mut Self,
                        _: &$device,
                        event: $module::Event,
                        _: &(),
                        _: &Connection,
                        _: &QueueHandle<Self>,
                    ) {
                        match event {
                            // arboard reads the clipboard itself, so the
                            // offers that come with these can go.
                            $module::Event::Selection { id } => {
                                if let Some(offer) = id {
                                    offer.destroy();
                                }
                                state.changed = true;
                            }
                            $module::Event::PrimarySelection { id: Some(offer) } => {
                                offer.destroy();
                            }
                            $module::Event::Finished => state.finished = true,
                            _ => {}
                        }
                    }

                    event_created_child!(State, $device, [
                        $module::EVT_DATA_OFFER_OPCODE => ($offer, ()),
                    ]);
                }
            };
        }

        handle_device!(
            ExtDataControlDeviceV1,
            ext_data_control_device_v1,
            ExtDataControlOfferV1
        );
        handle_device!(
            ZwlrDataControlDeviceV1,
            zwlr_data_control_device_v1,
            ZwlrDataControlOfferV1
        );
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSPasteboard;

    /// The types from nspasteboard.org, and the marker 1Password has long
    /// used.
    const MARKERS: [&str; 4] = [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
        "org.nspasteboard.AutoGeneratedType",
        "com.agilebits.onepassword",
    ];

    pub fn change_count() -> Option<u64> {
        Some(NSPasteboard::generalPasteboard().changeCount() as u64)
    }

    /// macOS doesn't announce clipboard changes; its change count is how
    /// they're found instead.
    pub struct Changes;

    impl Changes {
        pub fn listen() -> Option<Self> {
            None
        }

        pub fn name(&self) -> &'static str {
            unreachable!("macOS has no clipboard notifications")
        }

        pub fn wait(&mut self) -> bool {
            false
        }
    }

    pub fn is_concealed() -> bool {
        let Some(types) = NSPasteboard::generalPasteboard().types() else {
            return false;
        };
        types
            .to_vec()
            .iter()
            .any(|kind| MARKERS.contains(&kind.to_string().as_str()))
    }
}

#[cfg(windows)]
mod imp {
    use clipboard_win::monitor::Monitor;
    use clipboard_win::raw::{is_format_avail, register_format, seq_num};

    /// Formats that password managers add to keep a copy out of clipboard
    /// history. Apps set `CanIncludeInClipboardHistory` to 0 to opt out, and
    /// hardly ever set it at all otherwise, so its presence is enough.
    const MARKERS: [&str; 3] = [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "Clipboard Viewer Ignore",
    ];

    pub fn change_count() -> Option<u64> {
        seq_num().map(|number| number.get().into())
    }

    /// Windows sends a message to listeners registered with
    /// `AddClipboardFormatListener` whenever the clipboard changes.
    pub struct Changes(Monitor);

    impl Changes {
        pub fn listen() -> Option<Self> {
            Monitor::new().ok().map(Self)
        }

        pub fn name(&self) -> &'static str {
            "clipboard notifications"
        }

        pub fn wait(&mut self) -> bool {
            matches!(self.0.recv(), Ok(true))
        }
    }

    pub fn is_concealed() -> bool {
        MARKERS
            .iter()
            .filter_map(|name| register_format(name))
            .any(|format| is_format_avail(format.get()))
    }
}

#[cfg(not(any(
    all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ),
    target_os = "macos",
    windows
)))]
mod imp {
    pub fn change_count() -> Option<u64> {
        None
    }

    pub fn is_concealed() -> bool {
        false
    }

    pub struct Changes;

    impl Changes {
        pub fn listen() -> Option<Self> {
            None
        }

        pub fn name(&self) -> &'static str {
            unreachable!("no clipboard notifications here")
        }

        pub fn wait(&mut self) -> bool {
            false
        }
    }
}
