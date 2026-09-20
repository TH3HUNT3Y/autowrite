#![cfg_attr(not(windows), allow(dead_code))]
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Receiver,
        Arc,
    },
    thread,
    time::Duration,
};

const MINUTES_MIN: u64 = 10;
const MINUTES_MAX: u64 = 7 * 24 * 60;
const DEFAULT_TEXT: &str = "Paste text here, choose how long it should take, then place your cursor in the destination field.";

#[derive(Clone, Debug)]
enum TypingAction {
    KeyPress(char),
    Backspace,
    Pause(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerMessage {
    Pause,
    Resume,
    Stop,
}

#[derive(Clone)]
struct WorkerState {
    paused: Arc<AtomicBool>,
    canceled: Arc<AtomicBool>,
    completed: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            canceled: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn progress_percent(&self) -> usize {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            0
        } else {
            self.completed.load(Ordering::Relaxed) * 100 / total
        }
    }
}

fn adjacent_key(character: char) -> Option<char> {
    let lower = character.to_ascii_lowercase();
    let neighbor = match lower {
        'q' => 'w',
        'w' => 'e',
        'e' => 'r',
        'r' => 't',
        't' => 'y',
        'y' => 'u',
        'u' => 'i',
        'i' => 'o',
        'o' => 'p',
        'p' => 'o',
        'a' => 's',
        's' => 'd',
        'd' => 'f',
        'f' => 'g',
        'g' => 'h',
        'h' => 'j',
        'j' => 'k',
        'k' => 'l',
        'l' => 'k',
        'z' => 'x',
        'x' => 'c',
        'c' => 'v',
        'v' => 'b',
        'b' => 'n',
        'n' => 'm',
        'm' => 'n',
        _ => return None,
    };
    Some(if character.is_ascii_uppercase() {
        neighbor.to_ascii_uppercase()
    } else {
        neighbor
    })
}

struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        Self(seed ^ 0xa0761d6478bd642f)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 7;
        self.0 ^= self.0 >> 9;
        self.0 ^= self.0 << 8;
        self.0
    }

    fn unit(&mut self) -> f64 {
        self.next() as f64 / u64::MAX as f64
    }
    fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
    fn range(&mut self, minimum: u64, maximum: u64) -> u64 {
        minimum + self.next() % (maximum - minimum + 1)
    }
    fn normalish(&mut self, mean: f64, deviation: f64) -> f64 {
        mean + ((0..12).map(|_| self.unit()).sum::<f64>() - 6.0) * deviation
    }
}

fn humanize(text: &str, target_wpm: f64, typo_rate: f64) -> Vec<TypingAction> {
    let mut random = Random::new();
    let mean_ms = (60_000.0 / (target_wpm.max(1.0) * 5.0)).max(8.0);
    let mut actions = Vec::with_capacity(text.chars().count() * 2);
    let mut sentence_count = 0;
    for character in text.chars() {
        let mut delay = random.normalish(mean_ms, mean_ms * 0.22).max(8.0) as u64;
        if character == ',' {
            delay += 150;
        } else if matches!(character, '.' | '!' | '?' | ';' | ':') {
            delay += 300;
            sentence_count += 1;
            if sentence_count >= 2 && random.chance(0.18) {
                delay += random.range(1_000, 3_000);
                sentence_count = 0;
            }
        }
        actions.push(TypingAction::Pause(delay));
        if random.chance(typo_rate) {
            if let Some(wrong) = adjacent_key(character) {
                actions.push(TypingAction::KeyPress(wrong));
                actions.push(TypingAction::Pause(random.range(300, 600)));
                actions.push(TypingAction::Backspace);
            }
        }
        actions.push(TypingAction::KeyPress(character));
    }
    actions
}

fn scale_to_duration(actions: &mut [TypingAction], duration: Duration) {
    let current_ms: u128 = actions
        .iter()
        .map(|action| match action {
            TypingAction::Pause(milliseconds) => *milliseconds as u128,
            _ => 0,
        })
        .sum();
    if current_ms == 0 {
        return;
    }
    let scale = duration.as_millis() as f64 / current_ms as f64;
    for action in actions {
        if let TypingAction::Pause(milliseconds) = action {
            *milliseconds = ((*milliseconds as f64 * scale).round() as u64).max(1);
        }
    }
}

fn estimated_wpm(text: &str, duration_minutes: u64) -> f64 {
    let words = text.split_whitespace().count() as f64;
    if words == 0.0 {
        0.0
    } else {
        words / duration_minutes.max(1) as f64
    }
}

fn sleep_with_controls(
    milliseconds: u64,
    state: &WorkerState,
    receiver: &Receiver<WorkerMessage>,
) -> bool {
    let mut remaining = milliseconds;
    while remaining > 0 {
        drain_controls(state, receiver);
        if state.canceled.load(Ordering::Relaxed) {
            return false;
        }
        while state.paused.load(Ordering::Relaxed) {
            drain_controls(state, receiver);
            if state.canceled.load(Ordering::Relaxed) {
                return false;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let slice = remaining.min(100);
        thread::sleep(Duration::from_millis(slice));
        remaining -= slice;
    }
    true
}

fn drain_controls(state: &WorkerState, receiver: &Receiver<WorkerMessage>) {
    while let Ok(message) = receiver.try_recv() {
        match message {
            WorkerMessage::Pause => state.paused.store(true, Ordering::Relaxed),
            WorkerMessage::Resume => state.paused.store(false, Ordering::Relaxed),
            WorkerMessage::Stop => state.canceled.store(true, Ordering::Relaxed),
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        ffi::c_void,
        mem::size_of,
        ptr::null_mut,
        sync::mpsc::{self, Sender},
    };

    type Handle = *mut c_void;
    type Hwnd = Handle;
    type Hinstance = Handle;
    type Hmenu = Handle;
    type Lparam = isize;
    type Wparam = usize;
    type Lresult = isize;
    type Uint = u32;
    type Dword = u32;
    type Bool = i32;
    type Wndproc = unsafe extern "system" fn(Hwnd, Uint, Wparam, Lparam) -> Lresult;

    const CS_HREDRAW: Uint = 0x0002;
    const CS_VREDRAW: Uint = 0x0001;
    const WS_OVERLAPPEDWINDOW: Dword = 0x00cf0000;
    const WS_VISIBLE: Dword = 0x10000000;
    const WS_CHILD: Dword = 0x40000000;
    const WS_BORDER: Dword = 0x00800000;
    const ES_MULTILINE: Dword = 0x0004;
    const ES_AUTOVSCROLL: Dword = 0x0040;
    const ES_WANTRETURN: Dword = 0x1000;
    const ES_NUMBER: Dword = 0x2000;
    const BS_PUSHBUTTON: Dword = 0;
    const SW_SHOW: i32 = 5;
    const WM_CREATE: Uint = 1;
    const WM_DESTROY: Uint = 2;
    const WM_COMMAND: Uint = 0x0111;
    const WM_CLOSE: Uint = 0x0010;
    const WM_APP: Uint = 0x8000;
    const WM_APP_PROGRESS: Uint = WM_APP + 1;
    const GWLP_USERDATA: i32 = -21;
    const EM_SETLIMITTEXT: Uint = 0x00c5;
    const WM_GETTEXTLENGTH: Uint = 0x000e;
    const WM_GETTEXT: Uint = 0x000d;
    const VK_BACK: u16 = 0x08;
    const INPUT_KEYBOARD: Dword = 1;
    const KEYEVENTF_KEYUP: Dword = 0x0002;
    const KEYEVENTF_UNICODE: Dword = 0x0004;
    const IDC_ARROW: *const u16 = 32512usize as *const u16;
    const MB_OK: Uint = 0x0000;
    const MB_ICONERROR: Uint = 0x0010;

    #[repr(C)]
    struct WndclassW {
        style: Uint,
        wnd_proc: Wndproc,
        cls_extra: i32,
        wnd_extra: i32,
        instance: Hinstance,
        icon: Handle,
        cursor: Handle,
        background: Handle,
        menu_name: *const u16,
        class_name: *const u16,
    }
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Msg {
        hwnd: Hwnd,
        message: Uint,
        wparam: Wparam,
        lparam: Lparam,
        time: Uint,
        point: Point,
    }
    #[repr(C)]
    struct Keybdinput {
        virtual_key: u16,
        scan_code: u16,
        flags: Dword,
        time: Uint,
        extra_info: usize,
    }
    #[repr(C)]
    struct Input {
        input_type: Dword,
        keyboard: Keybdinput,
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(class: *const WndclassW) -> u16;
        fn CreateWindowExW(
            ex_style: Dword,
            class: *const u16,
            title: *const u16,
            style: Dword,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: Hwnd,
            menu: Hmenu,
            instance: Hinstance,
            param: *mut c_void,
        ) -> Hwnd;
        fn DefWindowProcW(hwnd: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Lresult;
        fn DispatchMessageW(message: *const Msg) -> Lresult;
        fn TranslateMessage(message: *const Msg) -> Bool;
        fn GetMessageW(message: *mut Msg, hwnd: Hwnd, min: Uint, max: Uint) -> i32;
        fn PostQuitMessage(exit_code: i32);
        fn PostMessageW(hwnd: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Bool;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> Bool;
        fn UpdateWindow(hwnd: Hwnd) -> Bool;
        fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: Lresult) -> Lresult;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> Lresult;
        fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> Bool;
        fn SendMessageW(hwnd: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Lresult;
        fn EnableWindow(hwnd: Hwnd, enable: Bool) -> Bool;
        fn SendInput(count: Uint, inputs: *const Input, size: i32) -> Uint;
        fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, flags: Uint) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> Hinstance;
        fn GetLastError() -> Dword;
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn get_text(hwnd: Hwnd) -> String {
        let length = unsafe { SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) } as usize;
        let mut buffer = vec![0u16; length + 1];
        unsafe {
            SendMessageW(
                hwnd,
                WM_GETTEXT,
                buffer.len(),
                buffer.as_mut_ptr() as Lparam,
            );
        }
        String::from_utf16_lossy(&buffer[..length])
    }
    fn set_text(hwnd: Hwnd, text: &str) {
        let value = wide(text);
        unsafe {
            SetWindowTextW(hwnd, value.as_ptr());
        }
    }

    fn show_startup_error(step: &str, error_code: Dword) {
        let message = wide(&format!(
            "Dripwriter could not start at {step}. Win32 error: {error_code}"
        ));
        let caption = wide("Dripwriter startup error");
        unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    fn button(
        parent: Hwnd,
        instance: Hinstance,
        label: &str,
        id: usize,
        x: i32,
        width: i32,
    ) -> Hwnd {
        let class = wide("BUTTON");
        let title = wide(label);
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
                x,
                390,
                width,
                30,
                parent,
                id as Hmenu,
                instance,
                null_mut(),
            )
        }
    }

    struct WindowState {
        text: Hwnd,
        duration: Hwnd,
        status: Hwnd,
        start: Hwnd,
        pause: Hwnd,
        stop: Hwnd,
        worker: Option<(Sender<WorkerMessage>, WorkerState)>,
    }

    unsafe extern "system" fn window_proc(
        hwnd: Hwnd,
        message: Uint,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Lresult {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
        match message {
            WM_CREATE => {
                let instance = GetModuleHandleW(null_mut());
                let edit_class = wide("EDIT");
                let static_class = wide("STATIC");
                let text_label = wide("Text to type");
                let duration_label = wide("Duration (minutes, 10-10080)");
                let text = CreateWindowExW(
                    WS_BORDER,
                    edit_class.as_ptr(),
                    wide(DEFAULT_TEXT).as_ptr(),
                    WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
                    18,
                    44,
                    644,
                    285,
                    hwnd,
                    101usize as Hmenu,
                    instance,
                    null_mut(),
                );
                SendMessageW(text, EM_SETLIMITTEXT, 1_000_000, 0);
                CreateWindowExW(
                    0,
                    static_class.as_ptr(),
                    text_label.as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    18,
                    18,
                    150,
                    22,
                    hwnd,
                    null_mut(),
                    instance,
                    null_mut(),
                );
                CreateWindowExW(
                    0,
                    static_class.as_ptr(),
                    duration_label.as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    18,
                    340,
                    180,
                    22,
                    hwnd,
                    null_mut(),
                    instance,
                    null_mut(),
                );
                let duration = CreateWindowExW(
                    WS_BORDER,
                    edit_class.as_ptr(),
                    wide("30").as_ptr(),
                    WS_CHILD | WS_VISIBLE | ES_NUMBER,
                    210,
                    337,
                    100,
                    25,
                    hwnd,
                    102usize as Hmenu,
                    instance,
                    null_mut(),
                );
                let status = CreateWindowExW(
                    0,
                    static_class.as_ptr(),
                    wide("Ready").as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    330,
                    340,
                    330,
                    22,
                    hwnd,
                    null_mut(),
                    instance,
                    null_mut(),
                );
                let start = button(hwnd, instance, "Start Dripping", 201, 18, 150);
                let pause = button(hwnd, instance, "Pause", 202, 180, 100);
                let stop = button(hwnd, instance, "Stop", 203, 298, 100);
                EnableWindow(pause, 0);
                EnableWindow(stop, 0);
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    Box::into_raw(Box::new(WindowState {
                        text,
                        duration,
                        status,
                        start,
                        pause,
                        stop,
                        worker: None,
                    })) as Lresult,
                );
                0
            }
            WM_COMMAND if !state_ptr.is_null() => {
                let state = &mut *state_ptr;
                match (wparam & 0xffff) as usize {
                    201 => start(hwnd, state),
                    202 => toggle_pause(state),
                    203 => stop(state),
                    _ => {}
                }
                0
            }
            WM_APP_PROGRESS if !state_ptr.is_null() => {
                let state = &mut *state_ptr;
                if let Some((_, worker)) = &state.worker {
                    let percent = worker.progress_percent();
                    let status = if worker.paused.load(Ordering::Relaxed) {
                        format!("Paused ({percent}%)")
                    } else {
                        format!("Running ({percent}%)")
                    };
                    set_text(state.status, &status);
                    if percent >= 100 || worker.canceled.load(Ordering::Relaxed) {
                        EnableWindow(state.start, 1);
                        EnableWindow(state.pause, 0);
                        EnableWindow(state.stop, 0);
                        set_text(
                            state.status,
                            if worker.canceled.load(Ordering::Relaxed) {
                                "Stopped"
                            } else {
                                "Complete"
                            },
                        );
                    }
                }
                0
            }
            WM_CLOSE => DefWindowProcW(hwnd, WM_CLOSE, wparam, lparam),
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn start(hwnd: Hwnd, state: &mut WindowState) {
        if state.worker.is_some() {
            return;
        }
        let text = get_text(state.text);
        if text.trim().is_empty() {
            set_text(state.status, "Enter text first");
            return;
        }
        let minutes = get_text(state.duration)
            .parse::<u64>()
            .unwrap_or(30)
            .clamp(MINUTES_MIN, MINUTES_MAX);
        set_text(state.duration, &minutes.to_string());
        let duration = Duration::from_secs(minutes * 60);
        let worker_state = WorkerState::new();
        let state_for_thread = worker_state.clone();
        let (sender, receiver) = mpsc::channel();
        let hwnd_value = hwnd as usize;
        let text_for_thread = text.clone();
        thread::spawn(move || {
            let mut actions = humanize(
                &text_for_thread,
                estimated_wpm(&text_for_thread, minutes).max(1.0),
                0.02,
            );
            scale_to_duration(&mut actions, duration);
            state_for_thread
                .total
                .store(actions.len(), Ordering::Relaxed);
            for action in actions {
                drain_controls(&state_for_thread, &receiver);
                if state_for_thread.canceled.load(Ordering::Relaxed) {
                    break;
                }
                match action {
                    TypingAction::Pause(milliseconds) => {
                        if !sleep_with_controls(milliseconds, &state_for_thread, &receiver) {
                            break;
                        }
                    }
                    TypingAction::KeyPress(character) => send_unicode(character),
                    TypingAction::Backspace => send_vk(VK_BACK),
                }
                state_for_thread.completed.fetch_add(1, Ordering::Relaxed);
                unsafe {
                    PostMessageW(hwnd_value as Hwnd, WM_APP_PROGRESS, 0, 0);
                }
            }
            unsafe {
                PostMessageW(hwnd_value as Hwnd, WM_APP_PROGRESS, 0, 0);
            }
        });
        state.worker = Some((sender, worker_state));
        unsafe {
            EnableWindow(state.start, 0);
            EnableWindow(state.pause, 1);
            EnableWindow(state.stop, 1);
        }
        set_text(state.status, "Running - focus destination field");
    }

    fn toggle_pause(state: &mut WindowState) {
        if let Some((sender, worker)) = &state.worker {
            let message = if worker.paused.load(Ordering::Relaxed) {
                WorkerMessage::Resume
            } else {
                WorkerMessage::Pause
            };
            let _ = sender.send(message);
            set_text(
                state.pause,
                if message == WorkerMessage::Pause {
                    "Resume"
                } else {
                    "Pause"
                },
            );
        }
    }
    fn stop(state: &mut WindowState) {
        if let Some((sender, worker)) = &state.worker {
            worker.canceled.store(true, Ordering::Relaxed);
            let _ = sender.send(WorkerMessage::Stop);
            set_text(state.status, "Stopping");
        }
    }
    fn send_vk(virtual_key: u16) {
        let inputs = [
            Input {
                input_type: INPUT_KEYBOARD,
                keyboard: Keybdinput {
                    virtual_key,
                    scan_code: 0,
                    flags: 0,
                    time: 0,
                    extra_info: 0,
                },
            },
            Input {
                input_type: INPUT_KEYBOARD,
                keyboard: Keybdinput {
                    virtual_key,
                    scan_code: 0,
                    flags: KEYEVENTF_KEYUP,
                    time: 0,
                    extra_info: 0,
                },
            },
        ];
        unsafe {
            SendInput(
                inputs.len() as Uint,
                inputs.as_ptr(),
                size_of::<Input>() as i32,
            );
        }
    }
    fn send_unicode(character: char) {
        let mut units = [0u16; 2];
        for unit in character.encode_utf16(&mut units) {
            let inputs = [
                Input {
                    input_type: INPUT_KEYBOARD,
                    keyboard: Keybdinput {
                        virtual_key: 0,
                        scan_code: *unit,
                        flags: KEYEVENTF_UNICODE,
                        time: 0,
                        extra_info: 0,
                    },
                },
                Input {
                    input_type: INPUT_KEYBOARD,
                    keyboard: Keybdinput {
                        virtual_key: 0,
                        scan_code: *unit,
                        flags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        extra_info: 0,
                    },
                },
            ];
            unsafe {
                SendInput(
                    inputs.len() as Uint,
                    inputs.as_ptr(),
                    size_of::<Input>() as i32,
                );
            }
        }
    }

    pub fn run() {
        unsafe {
            let instance = GetModuleHandleW(null_mut());
            let class_name = wide("DripwriterWindow");
            let class = WndclassW {
                style: CS_HREDRAW | CS_VREDRAW,
                wnd_proc: window_proc,
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: null_mut(),
                cursor: IDC_ARROW as Handle,
                background: 6usize as Handle,
                menu_name: null_mut(),
                class_name: class_name.as_ptr(),
            };
            if RegisterClassW(&class) == 0 {
                show_startup_error("window class registration", GetLastError());
                return;
            }
            let title = wide("Dripwriter");
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                700,
                480,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            if hwnd.is_null() {
                show_startup_error("window creation", GetLastError());
                return;
            }
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            let mut message = Msg {
                hwnd: null_mut(),
                message: 0,
                wparam: 0,
                lparam: 0,
                time: 0,
                point: Point { x: 0, y: 0 },
            };
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    native::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Dripwriter is a Windows-only native executable.");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_key_preserves_case() {
        assert_eq!(adjacent_key('a'), Some('s'));
        assert_eq!(adjacent_key('A'), Some('S'));
        assert_eq!(adjacent_key(' '), None);
    }
    #[test]
    fn duration_scaling_reaches_requested_pause_budget() {
        let mut actions = vec![
            TypingAction::Pause(100),
            TypingAction::KeyPress('a'),
            TypingAction::Pause(200),
        ];
        scale_to_duration(&mut actions, Duration::from_millis(6_000));
        let total: u64 = actions
            .iter()
            .map(|action| match action {
                TypingAction::Pause(milliseconds) => *milliseconds,
                _ => 0,
            })
            .sum();
        assert_eq!(total, 6_000);
    }
}
