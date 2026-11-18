//! Proceso principal: ícono en la bandeja, tecla rápida y control de la grabadora.
//! No carga nada de interfaz gráfica: los ajustes se abren en otro proceso (`--settings`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::autostart;
use super::keys::hotkey_name;
use super::status::{self, Status};
use super::mode::{Mode, Window, TRAY_CLASS, TRAY_MUTEX, WM_HOTKEY_SUSPEND, WM_REC_EVENT, WM_RELOAD, WM_TRAY, WM_UPDATE};
use super::update;
use crate::config::{self, Config, GlobalAction};
use crate::i18n::{tr, trf};
use crate::log::log;
use crate::record::recorder::{Event, Notify, Recorder};
use crate::ui_kit::{icon, sound, theme::fmt_secs, window::open_folder};


const ID_TOGGLE: usize = 10;
const ID_SAVE: usize = 11;
const ID_SETTINGS: usize = 12;
const ID_FOLDER: usize = 13;
const ID_EXIT: usize = 14;
const ID_EDITOR: usize = 15;
const ID_MIC: usize = 16;
const ID_CONTINUOUS: usize = 17;
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;

static EVENTS: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: RefCell<u32> = const { RefCell::new(0) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContinuousStatus {
    Idle,
    Starting,
    Recording,
    Finishing,
}

struct App {
    hwnd: HWND,
    cfg: Config,
    rec: Option<Recorder>,
    icon_on: HICON,
    icon_off: HICON,
    /// Con qué graba y por qué no puede (lo muestra también la ventana principal).
    status: Status,
    /// Juego detectado por la grabadora (para el tooltip).
    game: Option<String>,
    continuous: ContinuousStatus,
    continuous_since: Option<Instant>,
}

/// `background`: arrancar sin abrir la ventana (inicio automático con Windows).
pub fn run(background: bool) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Una sola instancia: si ya está corriendo, abrir sus ajustes.
        let _mutex = CreateMutexW(None, true, TRAY_MUTEX);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            open_settings();
            return;
        }

        update::cleanup();
        autostart::repair();
        let first_run = !config::exists();
        let cfg = config::load();
        if first_run {
            let _ = config::save(&cfg);
        }

        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: TRAY_CLASS, ..Default::default() };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            TRAY_CLASS,
            w!("RecorddyHost"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            hinst,
            None,
        ) else {
            log("no se pudo crear la ventana principal");
            return;
        };
        TASKBAR_CREATED.with(|t| *t.borrow_mut() = RegisterWindowMessageW(w!("TaskbarCreated")));

        let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
        let app = App {
            hwnd,
            cfg: Config { enabled: false, ..cfg.clone() },
            rec: None,
            icon_on: icon::hicon(size, true),
            icon_off: icon::hicon(size, false),
            status: Status::default(),
            game: None,
            continuous: ContinuousStatus::Idle,
            continuous_since: None,
        };
        app.tray(NIM_ADD);
        status::save(&app.status);
        APP.with(|a| *a.borrow_mut() = Some(app));
        with_app(|a| a.apply(cfg.clone(), true));
        log("Recorddy iniciado");
        if first_run {
            let key = hotkey_name(cfg.hotkey_vk, cfg.hotkey_mods);
            with_app(|a| {
                let secs = fmt_secs(cfg.seconds);
                a.balloon(
                    tr!("Recorddy está grabando", "Recorddy is recording", "O Recorddy está gravando"),
                    &trf!(
                        "Apretá {key} para guardar los últimos {secs}. Recorddy queda acá, en la bandeja.",
                        "Press {key} to save the last {secs}. Recorddy stays here, in the tray.",
                        "Aperte {key} para salvar os últimos {secs}. O Recorddy fica aqui, na bandeja."
                    ),
                    false,
                )
            });
        }
        if first_run || !background {
            open_settings();
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn notifier(hwnd: HWND) -> Notify {
    let h = hwnd.0 as isize;
    Arc::new(move |ev| {
        if let Ok(mut q) = EVENTS.lock() {
            q.push_back(ev);
        }
        unsafe {
            let _ = PostMessageW(HWND(h as *mut _), WM_REC_EVENT, WPARAM(0), LPARAM(0));
        }
    })
}

impl App {
    fn apply(&mut self, new: Config, startup: bool) {
        let restart = new.video_params() != self.cfg.video_params();
        let keys = |c: &Config| (GlobalAction::ALL.map(|a| c.hotkey(a)), c.enabled, c.continuous_mode);
        let hotkey_changed = keys(&new) != keys(&self.cfg);
        self.cfg = new;
        if restart || (self.cfg.enabled && self.rec.is_none()) {
            // El reinicio de la captura finaliza cualquier MP4 largo en progreso.
            self.continuous = ContinuousStatus::Idle;
            self.continuous_since = None;
            self.rec = None; // detiene la grabadora anterior
            self.set_status(Status::default());
            self.game = None;
            if self.cfg.enabled {
                if self.cfg.continuous_mode { self.continuous = ContinuousStatus::Starting; }
                self.rec = Some(Recorder::start(self.cfg.clone(), notifier(self.hwnd)));
            }
        } else if let Some(r) = &self.rec {
            // Duración, audio, carpeta: se aplican sin perder lo ya grabado.
            r.update(self.cfg.clone());
        }
        if hotkey_changed || startup {
            let failed = self.register_hotkeys();
            if !failed.is_empty() {
                let k = failed.join(", ");
                self.balloon(
                    tr!("Tecla ocupada", "Key in use", "Tecla em uso"),
                    &trf!(
                        "Otro programa está usando {k}. Elegí otra en Ajustes.",
                        "Another program is using {k}. Pick another one in Settings.",
                        "Outro programa está usando {k}. Escolha outra nas Configurações."
                    ),
                    true,
                );
            }
        }
        self.tray(NIM_MODIFY);
    }

    fn unregister_all(&self) {
        unsafe {
            for a in GlobalAction::ALL {
                let _ = UnregisterHotKey(self.hwnd, a.id());
            }
        }
    }

    /// Registra los atajos globales. Devuelve los nombres de los que no se pudieron usar.
    fn register_hotkeys(&mut self) -> Vec<String> {
        self.unregister_all();
        let mut failed = Vec::new();
        for a in GlobalAction::ALL {
            let (vk, mods) = self.cfg.hotkey(a);
            // Guardar clip solo mientras graba; los demás, siempre.
            if vk == 0 || (a == GlobalAction::SaveClip && (!self.cfg.enabled || self.cfg.continuous_mode)) {
                continue;
            }
            let ok = unsafe { RegisterHotKey(self.hwnd, a.id(), HOT_KEY_MODIFIERS(mods | MOD_NOREPEAT.0), vk).is_ok() };
            if !ok {
                failed.push(hotkey_name(vk, mods));
            }
        }
        failed
    }

    fn tip(&self) -> String {
        if let Some(e) = &self.status.error {
            return trf!("Recorddy · error: {e}", "Recorddy · error: {e}", "Recorddy · erro: {e}");
        }
        if self.cfg.continuous_mode && matches!(self.continuous, ContinuousStatus::Starting | ContinuousStatus::Recording | ContinuousStatus::Finishing) {
            return match self.continuous {
                ContinuousStatus::Starting => "Recorddy · iniciando grabación larga".into(),
                ContinuousStatus::Recording => "Recorddy · grabación larga activa".into(),
                ContinuousStatus::Finishing => "Recorddy · guardando grabación larga".into(),
                ContinuousStatus::Idle => unreachable!(),
            };
        }
        if self.cfg.enabled && self.cfg.continuous_mode {
            return "Recorddy · grabación continua iniciando".into();
        }
        if self.cfg.enabled {
            let (key, secs) = (hotkey_name(self.cfg.hotkey_vk, self.cfg.hotkey_mods), fmt_secs(self.cfg.seconds));
            let mut t = trf!("Recorddy · grabando · {key} guarda {secs}", "Recorddy · recording · {key} saves {secs}", "Recorddy · gravando · {key} salva {secs}");
            if let Some(g) = &self.game {
                t.push_str(&trf!(" · juego: {g}", " · game: {g}", " · jogo: {g}"));
            }
            t
        } else {
            tr!("Recorddy · pausado", "Recorddy · paused", "Recorddy · pausado").into()
        }
    }

    fn nid(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        }
    }

    fn tray(&self, op: NOTIFY_ICON_MESSAGE) {
        let mut n = self.nid();
        n.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        n.uCallbackMessage = WM_TRAY;
        n.hIcon = if self.cfg.enabled && self.status.error.is_none() { self.icon_on } else { self.icon_off };
        set_wstr(&mut n.szTip, &self.tip());
        unsafe {
            let _ = Shell_NotifyIconW(op, &n);
        }
    }

    fn balloon(&self, title: &str, text: &str, warn: bool) {
        let mut n = self.nid();
        n.uFlags = NIF_INFO;
        set_wstr(&mut n.szInfoTitle, title);
        set_wstr(&mut n.szInfo, text);
        n.dwInfoFlags = if warn { NIIF_WARNING } else { NIIF_INFO };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &n);
        }
    }

    /// Los modos son mutuamente excluyentes; cambiar de modo cierra el grabador anterior.
    fn select_mode(&mut self, continuous_mode: bool) {
        if self.continuous == ContinuousStatus::Finishing { return; }
        let mut c = self.cfg.clone();
        let same_mode = c.continuous_mode == continuous_mode;
        c.continuous_mode = continuous_mode;
        c.enabled = if same_mode { !c.enabled } else { true };
        let _ = config::save(&c);
        self.apply(c, false);
        if !continuous_mode && self.cfg.enabled { sound::play_resumed(); }
        if !continuous_mode && !self.cfg.enabled { sound::play_paused(); }
    }

    fn toggle_continuous(&mut self) {
        self.select_mode(true);
    }

    fn save_clip(&self) {
        if self.cfg.enabled && !self.cfg.continuous_mode {
            if let Some(r) = &self.rec { r.save(); }
        }
    }

    fn toggle(&mut self) {
        self.select_mode(false);
    }

    /// Cambia el estado y, si es otro, lo deja escrito para la ventana principal.
    fn set_status(&mut self, s: Status) {
        if s != self.status {
            self.status = s;
            status::save(&self.status);
        }
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Started(d) => {
                self.set_status(Status { device: Some(d), error: None });
                self.tray(NIM_MODIFY);
            }
            Event::Game(g) => {
                self.game = g;
                self.tray(NIM_MODIFY);
            }
            Event::Saved(_) => {} // el sonido lo reproduce la grabadora al instante
            Event::ContinuousStarted(p) => {
                if self.cfg.enabled && self.cfg.continuous_mode && self.cfg.sound_on_save {
                    sound::play_resumed();
                }
                if self.continuous == ContinuousStatus::Starting {
                    self.continuous = ContinuousStatus::Recording;
                    self.continuous_since = Some(Instant::now());
                }
                self.tray(NIM_MODIFY);
                self.balloon("Recorddy", &format!("Grabación continua iniciada: {}", p.display()), false);
            }
            Event::ContinuousSaved(p) => {
                if self.cfg.sound_on_save { sound::play_paused(); }
                self.continuous = ContinuousStatus::Idle;
                self.continuous_since = None;
                self.tray(NIM_MODIFY);
                self.balloon("Recorddy", &format!("Grabación guardada: {}", p.display()), false);
            }
            Event::ContinuousFailed(e) => {
                self.continuous = ContinuousStatus::Idle;
                self.continuous_since = None;
                if self.cfg.continuous_mode && self.cfg.enabled {
                    let mut c = self.cfg.clone();
                    c.enabled = false;
                    let _ = config::save(&c);
                    self.apply(c, false);
                }
                self.tray(NIM_MODIFY);
                self.balloon("Error en grabación continua", &e, true);
            },
            Event::SaveFailed(e) => self.balloon(tr!("No se pudo guardar el clip", "Couldn't save the clip", "Não foi possível salvar o clipe"), &e, true),
            Event::Error(e) => {
                // El globo sale al pasar de grabar a error, no si cambia el motivo mientras se reintenta.
                let new = self.status.error.is_none();
                self.set_status(Status { device: None, error: Some(e.clone()) });
                self.tray(NIM_MODIFY);
                if new {
                    self.balloon(
                        tr!("Recorddy no puede grabar", "Recorddy can't record", "O Recorddy não consegue gravar"),
                        &trf!("{e}\nSe reintenta solo.", "{e}\nIt retries by itself.", "{e}\nEle tenta de novo sozinho."),
                        true,
                    );
                }
            }
        }
    }

    /// Cierra todo: grabadora, editor y ventana principal.
    fn quit(&mut self) {
        Window::Editor.close();
        Window::Settings.close();
        self.rec = None;
        unsafe {
            self.unregister_all();
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid());
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn set_wstr(dst: &mut [u16], s: &str) {
    let v: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..v.len()].copy_from_slice(&v);
    dst[v.len()] = 0;
}

fn open_settings() {
    Mode::Settings.launch();
}

fn open_editor() {
    Mode::Editor(None).launch();
}

fn show_menu(hwnd: HWND) {
    let Some((enabled, key)) = with_app(|a| (a.cfg.enabled, hotkey_name(a.cfg.hotkey_vk, a.cfg.hotkey_mods))) else {
        return;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        // El editor va primero y en negrita: es lo que más se usa después de grabar.
        let item = |s: &str| HSTRING::from(s);
        let _ = AppendMenuW(menu, MF_STRING, ID_EDITOR, &item(tr!("Abrir editor de clips", "Open clip editor", "Abrir editor de clipes")));
        let _ = SetMenuDefaultItem(menu, ID_EDITOR as u32, 0);
        let save_label = HSTRING::from(trf!("Guardar clip ahora\t{key}", "Save clip now\t{key}", "Salvar clipe agora\t{key}"));
        let clips_active = with_app(|a| a.cfg.enabled && !a.cfg.continuous_mode).unwrap_or(false);
        let save_flags = if clips_active { MF_STRING } else { MF_STRING | MF_GRAYED };
        let _ = AppendMenuW(menu, save_flags, ID_SAVE, &save_label);
        let (continuous, elapsed, hotkey) = with_app(|a| {
            (a.continuous, a.continuous_since.map(|t| t.elapsed().as_secs()),
             hotkey_name(a.cfg.continuous_vk, a.cfg.continuous_mods))
        }).unwrap_or((ContinuousStatus::Idle, None, String::new()));
        let is_continuous_mode = with_app(|a| a.cfg.continuous_mode).unwrap_or(false);
        let label = if is_continuous_mode && enabled {
            format!("Grabación continua · Activa ({})", fmt_secs(elapsed.unwrap_or(0).min(u32::MAX as u64) as u32))
        } else {
            "Grabación continua · Inactiva".to_string()
        };
        let label = if hotkey.is_empty() || with_app(|a| a.cfg.continuous_vk == 0).unwrap_or(true) {
            label
        } else { format!("{label}\t{hotkey}") };
        let flags = if continuous == ContinuousStatus::Finishing { MF_STRING | MF_GRAYED }
            else { MF_STRING | if is_continuous_mode && enabled { MF_CHECKED } else { MF_UNCHECKED } };
        let _ = AppendMenuW(menu, flags, ID_CONTINUOUS, &HSTRING::from(label));
        let _ = AppendMenuW(menu, MF_STRING, ID_FOLDER, &item(tr!("Abrir carpeta de clips", "Open clips folder", "Abrir pasta de clipes")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let check = if clips_active { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | check, ID_TOGGLE, &item("Grabar clips"));
        let mic = with_app(|a| a.cfg.mic_audio).unwrap_or(false);
        let _ = AppendMenuW(menu, MF_STRING | if mic { MF_CHECKED } else { MF_UNCHECKED }, ID_MIC, &item(tr!("Micrófono", "Microphone", "Microfone")));
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, &item(tr!("Abrir Recorddy", "Open Recorddy", "Abrir Recorddy")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, &item(tr!("Salir", "Quit", "Sair")));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        run_command(cmd.0 as usize);
    }
}

fn run_command(id: usize) {
    match id {
        ID_TOGGLE => {
            with_app(|a| a.toggle());
        }
        ID_SAVE => {
            with_app(|a| a.save_clip());
        }
        ID_CONTINUOUS => {
            with_app(|a| a.toggle_continuous());
        }
        ID_SETTINGS => open_settings(),
        ID_EDITOR => open_editor(),
        ID_MIC => {
            with_app(|a| {
                let mut c = a.cfg.clone();
                c.mic_audio = !c.mic_audio;
                let _ = config::save(&c);
                a.apply(c, false);
            });
        }
        ID_FOLDER => open_clips_folder(),
        ID_EXIT => {
            with_app(|a| a.quit());
        }
        _ => {}
    }
}

fn open_clips_folder() {
    if let Some(d) = with_app(|a| a.cfg.output_dir.clone()) {
        open_folder(&d);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP | NIN_BALLOONUSERCLICK => open_settings(),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match GlobalAction::from_id(wp.0 as i32) {
                Some(GlobalAction::SaveClip) => {
                    with_app(|a| a.save_clip());
                }
                Some(GlobalAction::ToggleRecording) => { with_app(|a| a.toggle()); }
                Some(GlobalAction::OpenEditor) => open_editor(),
                Some(GlobalAction::OpenFolder) => open_clips_folder(),
                Some(GlobalAction::ToggleContinuous) => { with_app(|a| a.toggle_continuous()); },
                None => {}
            }
            LRESULT(0)
        }
        WM_UPDATE => {
            if let Some((v, _)) = update::available() {
                with_app(|a| {
                    a.balloon(
                        tr!("Hay una versión nueva de Recorddy", "A new version of Recorddy is out", "Há uma nova versão do Recorddy"),
                        &trf!(
                            "La versión {v} está lista. Tocá acá para actualizar.",
                            "Version {v} is ready. Click here to update.",
                            "A versão {v} está pronta. Clique aqui para atualizar."
                        ),
                        false,
                    )
                });
            }
            LRESULT(0)
        }
        WM_RELOAD => {
            let cfg = config::load();
            with_app(|a| a.apply(cfg, false));
            LRESULT(0)
        }
        WM_HOTKEY_SUSPEND => {
            with_app(|a| {
                if wp.0 == 1 {
                    a.unregister_all();
                } else {
                    a.register_hotkeys();
                }
            });
            LRESULT(0)
        }
        WM_REC_EVENT => {
            let evs: Vec<Event> = EVENTS.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
            for ev in evs {
                with_app(|a| a.handle_event(ev));
            }
            LRESULT(0)
        }
        // wparam 0: se canceló el apagado de Windows, así que se sigue grabando.
        WM_ENDSESSION if wp.0 == 0 => LRESULT(0),
        WM_CLOSE | WM_ENDSESSION => {
            // `taskkill /IM Recorddy.exe` o apagado de Windows: salir prolijo.
            with_app(|a| a.quit());
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            let tc = TASKBAR_CREATED.with(|t| *t.borrow());
            if tc != 0 && msg == tc {
                // Explorer se reinició: volver a poner el ícono.
                with_app(|a| a.tray(NIM_ADD));
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}
