//! Deadlock Counterpick: нативная оболочка на Tauri 2 для Windows, Linux и Android.
//!
//! Интерфейс целиком живёт в `dist/index.html` (тот же, что и веб-версия).
//! Rust добавляет то, чего нет в браузере:
//! - детектор игры на ПК: запущен ли Deadlock, чтение консольного лога (см. `game.rs`);
//! - глобальные горячие клавиши F6–F10 для таймеров (работают поверх игры, только ПК);
//! - режим «окно поверх игры» (только ПК);
//! - сохранение карточек статистики в папку «Изображения»;
//! - системные уведомления таймеров (ПК и Android);
//! - один экземпляр приложения и запоминание размера окна (только ПК).

mod game;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// Клавиши таймеров по порядку: мид-босс, большой лагерь, малый лагерь, урна, свой.
#[cfg(desktop)]
const TIMER_KEYS: [&str; 5] = ["F6", "F7", "F8", "F9", "F10"];

/// Состояние детектора, которое видит интерфейс (событие `game-status`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
struct GameStatus {
    /// Детектор работает (только ПК)
    supported: bool,
    running: bool,
    game_dir: Option<String>,
    log_found: bool,
    /// В лог писали за последние 2 минуты
    log_active: bool,
    match_id: Option<String>,
    /// Внутренние имена героев, замеченные в текущем матче (hero_inferno и т. п.)
    heroes: Vec<String>,
    /// Номер только что завершённого матча
    last_match_id: Option<String>,
}

#[derive(Default)]
struct GameShared {
    status: Mutex<GameStatus>,
    ring: Mutex<VecDeque<String>>,
    dir_override: Mutex<Option<PathBuf>>,
}

/// Сохраняет PNG-карточку (base64 из интерфейса) и возвращает путь к файлу.
#[tauri::command]
fn save_card(app: tauri::AppHandle, name: String, data_b64: String) -> Result<String, String> {
    use base64::Engine;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("неверные данные картинки: {e}"))?;

    let base = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().download_dir())
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| format!("не нашёл папку для сохранения: {e}"))?;
    let dir = base.join("Deadlock Counterpick");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Оставляем в имени только безопасные символы
    let mut file: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if !file.ends_with(".png") {
        file = "card.png".into();
    }

    // Не перезаписываем старые карточки: deadlock-week.png, deadlock-week-2.png, ...
    let stem = file.trim_end_matches(".png").to_string();
    let mut path = dir.join(&file);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.png"));
        n += 1;
    }

    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Включает или выключает режим «окно поверх других окон».
#[tauri::command]
fn set_on_top(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    apply_on_top(&window, on)
}

#[cfg(desktop)]
fn apply_on_top(window: &tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

#[cfg(mobile)]
fn apply_on_top(_window: &tauri::WebviewWindow, _on: bool) -> Result<(), String> {
    Ok(())
}

/// Текущее состояние детектора игры.
#[tauri::command]
fn game_status(state: tauri::State<'_, GameShared>) -> GameStatus {
    state.status.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Строки лога с ключевыми словами: для диагностики и настройки детектора.
#[tauri::command]
fn game_log_sample(state: tauri::State<'_, GameShared>) -> String {
    state
        .ring
        .lock()
        .map(|r| r.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Ручной путь к папке игры (если автопоиск не нашёл). Пустая строка сбрасывает.
#[tauri::command]
fn set_game_dir(state: tauri::State<'_, GameShared>, path: Option<String>) -> Result<bool, String> {
    let dir = path
        .map(|p| p.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|p| !p.is_empty())
        .map(|p| match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
            _ => PathBuf::from(p),
        });
    let ok = dir.as_ref().map(|d| game::is_game_dir(d)).unwrap_or(true);
    if !ok {
        let example = if cfg!(windows) {
            "D:\\SteamLibrary\\steamapps\\common\\Deadlock"
        } else {
            "~/.local/share/Steam/steamapps/common/Deadlock"
        };
        return Err(format!(
            "В этой папке нет game/citadel. Укажи папку Deadlock, например {example}"
        ));
    }
    *state.dir_override.lock().map_err(|e| e.to_string())? = dir;
    Ok(true)
}

/// Фоновый цикл детектора. Пока игра запущена или пишет лог: лог раз в секунду, процесс раз в 3 секунды.
/// Когда игры нет: просыпаемся раз в 2 секунды, процесс проверяем раз в 5 секунд.
#[cfg(desktop)]
fn game_loop(app: tauri::AppHandle) {
    use std::time::{Duration, Instant};

    let mut tail: Option<game::LogTail> = None;
    let mut det = game::Detector::default();
    let mut game_dir: Option<PathBuf> = None;
    let mut auto_dir = true;
    let mut running = false;
    let mut last_proc: Option<Instant> = None;
    let mut last_search: Option<Instant> = None;
    let mut last_line: Option<Instant> = None;
    let mut prev = GameStatus::default();

    loop {
        let shared = app.state::<GameShared>();

        // Папка игры: ручная настройка важнее автопоиска
        let over = shared.dir_override.lock().ok().and_then(|o| o.clone());
        if over.is_some() && over != game_dir {
            game_dir = over;
            auto_dir = false;
            tail = None;
        } else if over.is_none() && !auto_dir {
            game_dir = None;
            auto_dir = true;
            tail = None;
        }
        if game_dir.is_none() && last_search.map_or(true, |t| t.elapsed() > Duration::from_secs(30))
        {
            game_dir = game::find_game_dir();
            last_search = Some(Instant::now());
        }

        // Процесс игры
        let active = running || last_line.map_or(false, |t| t.elapsed() < Duration::from_secs(120));
        let proc_every = Duration::from_secs(if running { 3 } else { 5 });
        if last_proc.map_or(true, |t| t.elapsed() >= proc_every) {
            let r = game::is_game_running();
            if r != running {
                running = r;
                if !running {
                    det.end_session();
                }
            }
            last_proc = Some(Instant::now());
        }

        // Лог
        if let Some(dir) = &game_dir {
            let lp = game::log_path(dir);
            if tail.as_ref().map_or(true, |t| t.path != lp) {
                // Если игра уже идёт, читаем текущую сессию с начала, иначе только новое
                tail = Some(game::LogTail::new(lp, !running));
            }
        }
        if let Some(t) = tail.as_mut() {
            if let Ok(lines) = t.read_new() {
                if !lines.is_empty() {
                    last_line = Some(Instant::now());
                    for l in &lines {
                        det.feed(l);
                    }
                    if let Ok(mut ring) = shared.ring.lock() {
                        *ring = det.ring.clone();
                    }
                }
            }
        }

        let st = GameStatus {
            supported: true,
            running,
            game_dir: game_dir.as_ref().map(|d| d.display().to_string()),
            log_found: tail.as_ref().map_or(false, |t| t.path.exists()),
            log_active: last_line.map_or(false, |t| t.elapsed() < Duration::from_secs(120)),
            match_id: det.session.match_id.map(|m| m.to_string()),
            heroes: det.session.heroes.clone(),
            last_match_id: det.last_match.map(|m| m.to_string()),
        };
        if st != prev {
            if let Ok(mut s) = shared.status.lock() {
                *s = st.clone();
            }
            let _ = app.emit("game-status", &st);
            prev = st;
        }
        std::thread::sleep(Duration::from_millis(if active { 1000 } else { 2000 }));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};

        builder = builder
            // Повторный запуск не открывает второе окно, а показывает первое
            .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }))
            .plugin(tauri_plugin_window_state::Builder::default().build())
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, shortcut, event| {
                        if event.state() != ShortcutState::Pressed {
                            return;
                        }
                        let codes = [Code::F6, Code::F7, Code::F8, Code::F9, Code::F10];
                        if let Some(i) = codes
                            .iter()
                            .position(|c| shortcut.matches(Modifiers::empty(), *c))
                        {
                            let _ = app.emit("timer-hotkey", i);
                        }
                    })
                    .build(),
            );
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .manage(GameShared::default())
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                // Если клавиша занята другой программой, просто пропускаем её
                for key in TIMER_KEYS {
                    if let Err(e) = app.global_shortcut().register(key) {
                        eprintln!("не удалось занять клавишу {key}: {e}");
                    }
                }
                let handle = app.handle().clone();
                std::thread::spawn(move || game_loop(handle));
            }
            #[cfg(mobile)]
            let _ = app;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            save_card,
            set_on_top,
            game_status,
            game_log_sample,
            set_game_dir
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Deadlock Counterpick");
}
