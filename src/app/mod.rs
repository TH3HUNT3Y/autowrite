use crate::{
    config::settings::Settings,
    typing::{EngineCommand, EngineEvent, TypingEngine},
};
use std::ffi::c_void;
use windows::{
    core::{HSTRING, w},
    Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    Win32::System::LibraryLoader::GetModuleHandleW,
    Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowTextLengthW, GetWindowTextW, PostQuitMessage, RegisterClassExW, SetTimer,
        SetWindowTextW, ShowWindow, TranslateMessage, BN_CLICKED, CREATESTRUCTW, CS_HREDRAW,
        CS_VREDRAW, CW_USEDEFAULT, HMENU, IDC_ARROW, MSG, SW_SHOW, WM_CLOSE, WM_COMMAND,
        WM_CREATE, WM_DESTROY, WM_TIMER, WINDOW_STYLE, WNDCLASSEXW, WS_CHILD,
        WS_EX_CLIENTEDGE, WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
        WS_VSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
    },
};

const ID_SOURCE: u32 = 1001;
const ID_STATUS: u32 = 1002;
const ID_START: u32 = 1003;
const ID_PAUSE: u32 = 1004;
const ID_STOP: u32 = 1005;
const TIMER_POLL: usize = 1;

pub struct AppState {
    commands: crossbeam_channel::Sender<EngineCommand>,
    events: crossbeam_channel::Receiver<EngineEvent>,
    settings: Settings,
    source_edit: Option<HWND>,
    status_label: Option<HWND>,
    start_button: Option<HWND>,
    pause_button: Option<HWND>,
    stop_button: Option<HWND>,
    status: String,
    running: bool,
    paused: bool,
}

impl AppState {
    fn new() -> Self {
        let (events_sender, events) = crossbeam_channel::unbounded();
        let commands = TypingEngine::spawn(events_sender);
        crate::input::windows::spawn_hotkeys(commands.clone());

        Self {
            commands,
            events,
            settings: Settings::load(),
            source_edit: None,
            status_label: None,
            start_button: None,
            pause_button: None,
            stop_button: None,
            status: "Ready".to_string(),
            running: false,
            paused: false,
        }
    }

    fn set_status(&mut self, value: &str) {
        self.status = value.to_string();
        if let Some(hwnd) = self.status_label {
            let text = HSTRING::from(value);
            unsafe {
                let _ = SetWindowTextW(hwnd, &text);
            }
        }
    }

    fn read_source_text(&self) -> String {
        let Some(edit) = self.source_edit else {
            return String::new();
        };

        let length = unsafe { GetWindowTextLengthW(edit) as usize };
        if length == 0 {
            return String::new();
        }

        let mut buffer = vec![0u16; length + 1];
        let copied = unsafe { GetWindowTextW(edit, &mut buffer) } as usize;
        if copied == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..copied]).trim().to_string()
        }
    }

    fn start_typing(&mut self) {
        let text = self.read_source_text();
        if text.trim().is_empty() {
            self.set_status("Enter text first");
            return;
        }

        let _ = self.commands.send(EngineCommand::Start(text, self.settings.clone()));
        self.running = true;
        self.paused = false;
        self.set_status("Typing");
    }

    fn toggle_pause(&mut self) {
        if !self.running {
            return;
        }

        self.paused = !self.paused;
        let _ = self.commands.send(if self.paused {
            EngineCommand::Pause
        } else {
            EngineCommand::Resume
        });
        self.set_status(if self.paused { "Paused" } else { "Typing" });
    }

    fn stop_typing(&mut self) {
        if !self.running {
            self.set_status("Ready");
            return;
        }

        let _ = self.commands.send(EngineCommand::Stop);
        self.running = false;
        self.paused = false;
        self.set_status("Stopped");
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                EngineEvent::State(status) => self.set_status(&status),
                EngineEvent::Stats(stats) => {
                    if stats.finished {
                        self.running = false;
                        self.paused = false;
                    }
                    if !self.running && !self.paused && self.status != "Stopped" {
                        self.set_status("Finished");
                    }
                }
                EngineEvent::Error(error) => self.set_status(&error),
            }
        }
    }
}

pub fn run() {
    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let instance = HINSTANCE(instance.0);
        let class_name = w!("AutoWriteWindowClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, IDC_ARROW)
                .unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            return;
        }

        let state = Box::new(AppState::new());
        let state_ptr = Box::into_raw(state);

        let hw = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            class_name,
            w!("Auto Write"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200,
            780,
            None,
            None,
            Some(instance),
            Some(state_ptr as *const c_void),
        );

        if let Ok(hwnd) = hw {
            ShowWindow(hwnd, SW_SHOW);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        unsafe {
            let _ = Box::from_raw(state_ptr);
        }
    }
}

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let state = create.lpCreateParams as *mut AppState;
            let instance = GetModuleHandleW(None).expect("module handle");
            let state_ref = &mut *state;

            if let Ok(edit) = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                w!(""),
                WINDOW_STYLE(
                    WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0
                        | WS_VSCROLL.0
                        | WS_HSCROLL.0
                        | ES_MULTILINE as u32
                        | ES_AUTOVSCROLL as u32,
                ),
                16,
                70,
                1120,
                560,
                Some(window),
                Some(HMENU(ID_SOURCE as *mut c_void)),
                Some(instance.into()),
                None,
            ) {
                state_ref.source_edit = Some(edit);
            }

            if let Ok(status) = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("STATIC"),
                w!("Ready"),
                WS_CHILD | WS_VISIBLE,
                16,
                16,
                290,
                24,
                Some(window),
                Some(HMENU(ID_STATUS as *mut c_void)),
                Some(instance.into()),
                None,
            ) {
                state_ref.status_label = Some(status);
            }

            if let Ok(button) = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("BUTTON"),
                w!("START"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                340,
                18,
                120,
                32,
                Some(window),
                Some(HMENU(ID_START as *mut c_void)),
                Some(instance.into()),
                None,
            ) {
                state_ref.start_button = Some(button);
            }

            if let Ok(button) = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("BUTTON"),
                w!("PAUSE"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                480,
                18,
                120,
                32,
                Some(window),
                Some(HMENU(ID_PAUSE as *mut c_void)),
                Some(instance.into()),
                None,
            ) {
                state_ref.pause_button = Some(button);
            }

            if let Ok(button) = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("BUTTON"),
                w!("STOP"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                620,
                18,
                120,
                32,
                Some(window),
                Some(HMENU(ID_STOP as *mut c_void)),
                Some(instance.into()),
                None,
            ) {
                state_ref.stop_button = Some(button);
            }

            SetTimer(Some(window), TIMER_POLL, 100, None);
            return LRESULT(0);
        }
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u32;
            let notification = ((wparam.0 >> 16) & 0xFFFF) as u16;
            if notification == BN_CLICKED as u16 {
                let state = (lparam.0 as *mut AppState).as_mut();
                if let Some(state) = state {
                    match control_id {
                        ID_START => state.start_typing(),
                        ID_PAUSE => state.toggle_pause(),
                        ID_STOP => state.stop_typing(),
                        _ => {}
                    }
                }
            }
            return DefWindowProcW(window, message, wparam, lparam);
        }
        WM_TIMER => {
            let state = (lparam.0 as *mut AppState).as_mut();
            if let Some(state) = state {
                state.poll_events();
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            unsafe {
                DestroyWindow(window);
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            return LRESULT(0);
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
