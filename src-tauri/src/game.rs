//! Детектор игры Deadlock на ПК.
//!
//! Читает только обычные файлы, которые игра пишет сама, и список процессов.
//! Никакого чтения памяти игры, поэтому это безопасно для аккаунта.
//!
//! Источник данных: консольный лог `game/citadel/console.log`. Игра пишет его,
//! если в параметрах запуска Steam указан `-condebug`.
//! Формат строк Valve не документирует, поэтому распознавание устроено
//! по устойчивым признакам:
//! - внутренние имена героев вида `hero_inferno`;
//! - номер матча рядом со словами `match id`;
//! - загрузка карты как граница между матчами.
//!
//! Модуль не зависит от внешних библиотек, чтобы логику можно было проверить
//! обычными тестами: `rustc --edition 2021 --test game.rs`.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Сколько строк диагностики хранить.
const RING_MAX: usize = 300;
/// Максимум байт, читаемых из лога за один раз (на случай огромного файла).
const MAX_READ: u64 = 1 << 20;

const RESET_MARKERS: [&str; 4] = ["loading map", "changelevel", "map load", "loaded map"];
const INTERESTING: [&str; 12] = [
    "hero_",
    "match",
    "lobby",
    "team",
    "map",
    "connect",
    "spawn",
    "victory",
    "defeat",
    "game over",
    "citadel_",
    "player",
];

/// Что удалось найти в одной строке лога.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct LineInfo {
    pub heroes: Vec<String>,
    pub match_id: Option<u64>,
    pub reset: bool,
    pub interesting: bool,
}

fn is_word(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
}

fn find(hay: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if pat.is_empty() || from >= hay.len() || hay.len() - from < pat.len() {
        return None;
    }
    hay[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

/// Разбирает одну строку лога. Работает с байтами, поэтому не падает на кириллице.
pub fn extract(line: &str) -> LineInfo {
    let low = line.to_ascii_lowercase();
    let b = low.as_bytes();
    let mut info = LineInfo::default();

    // Внутренние имена героев: hero_xxx на границе слова
    let mut i = 0;
    while let Some(start) = find(b, b"hero_", i) {
        let mut end = start + 5;
        while end < b.len() && is_word(b[end]) {
            end += 1;
        }
        let left_ok = start == 0 || !is_word(b[start - 1]);
        if left_ok && end > start + 5 {
            let name = String::from_utf8_lossy(&b[start..end]).into_owned();
            if !info.heroes.contains(&name) {
                info.heroes.push(name);
            }
        }
        i = end;
    }

    // Номер матча: "match" → в пределах 24 байт "id" → 7–12 цифр
    let mut j = 0;
    while let Some(p) = find(b, b"match", j) {
        let s = p + 5;
        let w_end = (s + 24).min(b.len());
        if let Some(k) = find(&b[..w_end], b"id", s) {
            let mut d = k + 2;
            let mut skipped = 0;
            while d < b.len() && !b[d].is_ascii_digit() && skipped < 6 {
                d += 1;
                skipped += 1;
            }
            let ds = d;
            while d < b.len() && b[d].is_ascii_digit() {
                d += 1;
            }
            let len = d - ds;
            let right_ok = d >= b.len() || !b[d].is_ascii_alphanumeric();
            if (7..=12).contains(&len) && right_ok {
                if let Ok(id) = low[ds..d].parse::<u64>() {
                    info.match_id = Some(id);
                    break;
                }
            }
        }
        j = s;
    }

    info.reset = RESET_MARKERS.iter().any(|m| low.contains(m));
    info.interesting = INTERESTING.iter().any(|m| low.contains(m));
    info
}

/// Текущая сессия (матч): кто замечен и номер матча.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Session {
    pub heroes: Vec<String>,
    pub match_id: Option<u64>,
}

/// Накопитель событий из лога.
#[derive(Default, Debug)]
pub struct Detector {
    pub session: Session,
    /// Номер последнего завершённого матча (чтобы сразу открыть разбор).
    pub last_match: Option<u64>,
    pub ring: VecDeque<String>,
}

impl Detector {
    /// Конец матча: запоминаем номер и очищаем сессию.
    pub fn end_session(&mut self) {
        if let Some(m) = self.session.match_id {
            self.last_match = Some(m);
        }
        self.session = Session::default();
    }

    /// Обрабатывает строку. Возвращает true, если сессия изменилась.
    pub fn feed(&mut self, line: &str) -> bool {
        let info = extract(line);
        let mut changed = false;
        if info.reset && self.session != Session::default() {
            self.end_session();
            changed = true;
        }
        for h in info.heroes {
            if !self.session.heroes.contains(&h) {
                self.session.heroes.push(h);
                changed = true;
            }
        }
        if let Some(m) = info.match_id {
            if self.session.match_id != Some(m) {
                self.session.match_id = Some(m);
                changed = true;
            }
        }
        if info.interesting {
            if self.ring.len() >= RING_MAX {
                self.ring.pop_front();
            }
            self.ring.push_back(line.chars().take(300).collect());
        }
        changed
    }
}

/// Чтение только новых строк растущего файла.
pub struct LogTail {
    pub path: PathBuf,
    offset: u64,
    partial: String,
}

impl LogTail {
    /// `from_end`: начать с конца файла (старые строки не читать).
    pub fn new(path: PathBuf, from_end: bool) -> Self {
        let offset = if from_end {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        LogTail {
            path,
            offset,
            partial: String::new(),
        }
    }

    pub fn read_new(&mut self) -> std::io::Result<Vec<String>> {
        let mut f = File::open(&self.path)?;
        let len = f.metadata()?.len();
        if len < self.offset {
            // Игра перезапущена и перезаписала лог
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }
        if len - self.offset > MAX_READ {
            self.offset = len - MAX_READ;
            self.partial.clear();
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        f.take(len - self.offset).read_to_end(&mut buf)?;
        self.offset = len;
        let mut text = std::mem::take(&mut self.partial);
        text.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        self.partial = lines.pop().unwrap_or_default();
        Ok(lines.into_iter().filter(|l| !l.is_empty()).collect())
    }
}

/// Пути библиотек Steam из `libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let t = l.trim();
            if !t.to_ascii_lowercase().starts_with("\"path\"") {
                return None;
            }
            let rest = &t[6..];
            let q1 = rest.find('"')?;
            let rest = &rest[q1 + 1..];
            let q2 = rest.rfind('"')?;
            Some(PathBuf::from(rest[..q2].replace("\\\\", "\\")))
        })
        .collect()
}

/// Путь к консольному логу внутри папки игры.
pub fn log_path(game_dir: &Path) -> PathBuf {
    game_dir.join("game").join("citadel").join("console.log")
}

/// Похоже ли это на папку Deadlock.
pub fn is_game_dir(dir: &Path) -> bool {
    dir.join("game").join("citadel").is_dir()
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        if let Some(p) = registry_steam_path() {
            roots.push(p);
        }
        roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        roots.push(PathBuf::from(r"C:\Program Files\Steam"));
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let h = PathBuf::from(home);
            roots.push(h.join(".steam/steam"));
            roots.push(h.join(".local/share/Steam"));
            roots.push(h.join(".steam/root"));
            // Steam из Flatpak и Snap
            roots.push(h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            roots.push(h.join("snap/steam/common/.local/share/Steam"));
        }
    }
    roots
}

/// Ищет папку Deadlock во всех библиотеках Steam.
pub fn find_game_dir() -> Option<PathBuf> {
    let mut libs = Vec::new();
    for root in steam_roots() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            libs.extend(parse_library_folders(&text));
        }
        libs.push(root);
    }
    #[cfg(windows)]
    for letter in b'C'..=b'H' {
        libs.push(PathBuf::from(format!("{}:\\SteamLibrary", letter as char)));
    }
    // Одна и та же библиотека часто видна по нескольким путям (симлинки ~/.steam/steam)
    let mut seen = std::collections::HashSet::new();
    libs.retain(|l| seen.insert(std::fs::canonicalize(l).unwrap_or_else(|_| l.clone())));
    libs.into_iter()
        .map(|l| l.join("steamapps").join("common").join("Deadlock"))
        .find(|d| is_game_dir(d))
}

#[cfg(windows)]
fn hidden_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// Путь Steam из реестра. Читаем один раз за запуск: иначе каждые 30 секунд поиска
/// папки игры запускался бы процесс reg.exe.
#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHE.get_or_init(read_registry_steam_path).clone()
}

#[cfg(windows)]
fn read_registry_steam_path() -> Option<PathBuf> {
    let out = hidden_command("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("REG_SZ"))?;
    let value = line.split("REG_SZ").nth(1)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value.replace('/', "\\")))
    }
}

/// Похож ли процесс на игру. На Linux Deadlock идёт через Proton,
/// поэтому смотрим и имя процесса, и командную строку (там путь к deadlock.exe).
/// Сам Deadlock Counterpick («deadlock-counterpick») сюда не попадает.
pub fn is_deadlock_process(comm: &str, cmdline: &str) -> bool {
    let comm = comm.trim().to_ascii_lowercase();
    if comm == "deadlock.exe" || comm == "deadlock" {
        return true;
    }
    // Аргументы в /proc/<pid>/cmdline разделены нулевыми байтами
    cmdline
        .split('\0')
        .filter(|a| !a.is_empty())
        .take(3)
        .any(|a| {
            let a = a.to_ascii_lowercase().replace('\\', "/");
            let file = a.rsplit('/').next().unwrap_or("");
            file == "deadlock.exe"
        })
}

/// Список процессов Windows напрямую через Toolhelp32 (kernel32), без запуска tasklist.exe.
/// Раньше каждые 3 секунды создавался новый процесс с консолью: это десятки миллисекунд
/// процессорного времени и лишняя нагрузка на систему прямо во время игры.
#[cfg(windows)]
mod winproc {
    use std::ffi::c_void;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessEntry32W {
        dwSize: u32,
        cntUsage: u32,
        th32ProcessID: u32,
        th32DefaultHeapID: usize,
        th32ModuleID: u32,
        cntThreads: u32,
        th32ParentProcessID: u32,
        pcPriClassBase: i32,
        dwFlags: u32,
        szExeFile: [u16; 260],
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// Есть ли процесс с таким именем файла. `None`, если снимок процессов недоступен.
    pub fn process_exists(exe: &str) -> Option<bool> {
        // SAFETY: структура повторяет PROCESSENTRY32W из tlhelp32.h, dwSize задан до вызова,
        // дескриптор снимка закрывается на всех путях после успешного создания.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut e: ProcessEntry32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
            let mut found = false;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                    found = true;
                    break;
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            Some(found)
        }
    }
}

/// Запущена ли игра (процесс deadlock.exe).
pub fn is_game_running() -> bool {
    #[cfg(windows)]
    {
        if let Some(r) = winproc::process_exists("deadlock.exe") {
            return r;
        }
        // Запасной путь, если снимок процессов почему-то недоступен
        hidden_command("tasklist")
            .args(["/FI", "IMAGENAME eq deadlock.exe", "/FO", "CSV", "/NH"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_ascii_lowercase()
                    .contains("deadlock.exe")
            })
            .unwrap_or(false)
    }
    #[cfg(target_os = "linux")]
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        // PID найденной игры: пока она жива, проверяем только его, без обхода всех процессов
        static LAST_PID: AtomicU32 = AtomicU32::new(0);
        let check = |dir: &Path| {
            let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
            let cmd = std::fs::read(dir.join("cmdline")).unwrap_or_default();
            is_deadlock_process(&comm, &String::from_utf8_lossy(&cmd))
        };
        let last = LAST_PID.load(Ordering::Relaxed);
        if last != 0 && check(&Path::new("/proc").join(last.to_string())) {
            return true;
        }
        LAST_PID.store(0, Ordering::Relaxed);
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(pid) = name.to_str().and_then(|p| p.parse::<u32>().ok()) else {
                continue;
            };
            if check(&e.path()) {
                LAST_PID.store(pid, Ordering::Relaxed);
                return true;
            }
        }
        false
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlock_process_detected_under_proton() {
        assert!(is_deadlock_process("deadlock.exe\n", ""));
        assert!(is_deadlock_process(
            "wine64-preload",
            "Z:\\home\\dan\\.local\\share\\Steam\\steamapps\\common\\Deadlock\\game\\bin\\win64\\deadlock.exe\0-condebug\0"
        ));
        assert!(is_deadlock_process(
            "main",
            "/home/dan/.steam/steam/steamapps/common/Deadlock/game/bin/win64/deadlock.exe\0-condebug"
        ));
        // Само приложение, Steam и прочее не считаются игрой
        assert!(!is_deadlock_process(
            "deadlock-counte",
            "/usr/bin/deadlock-counterpick\0"
        ));
        assert!(!is_deadlock_process(
            "steam",
            "steam\0-applaunch\0deadlock.exe.log\0"
        ));
        assert!(!is_deadlock_process(
            "bash",
            "bash\0-c\0echo\0x\0deadlock.exe"
        ));
    }

    #[test]
    fn heroes_found_on_word_boundary() {
        let i = extract("[Client] Precache hero_inferno, hero_ATLAS and superhero_x; hero_ alone");
        assert_eq!(i.heroes, vec!["hero_inferno", "hero_atlas"]);
    }

    #[test]
    fn heroes_dedup_and_cyrillic_safe() {
        let i = extract("Игрок Даниэль выбрал hero_haze (hero_haze) — ок");
        assert_eq!(i.heroes, vec!["hero_haze"]);
    }

    #[test]
    fn match_id_variants() {
        assert_eq!(extract("Match ID: 41234567").match_id, Some(41234567));
        assert_eq!(
            extract("citadel match_id=3912345678 started").match_id,
            Some(3912345678)
        );
        assert_eq!(extract("[GC] matchid 12345678").match_id, Some(12345678));
        assert_eq!(extract("match id 123").match_id, None); // слишком короткий
        assert_eq!(extract("rematch idle 99999999x").match_id, None); // цифры с буквой
        assert_eq!(extract("no ids here 12345678").match_id, None);
    }

    #[test]
    fn reset_and_interesting() {
        assert!(extract("Loading map \"street_test\"").reset);
        assert!(!extract("player disconnected").reset);
        assert!(extract("Lobby update").interesting);
        assert!(!extract("Texture streaming budget").interesting);
    }

    #[test]
    fn detector_sessions() {
        let mut d = Detector::default();
        assert!(d.feed("Match ID: 55555555"));
        assert!(d.feed("spawn hero_wraith"));
        assert!(!d.feed("spawn hero_wraith again"));
        assert_eq!(d.session.heroes, vec!["hero_wraith"]);
        assert!(d.feed("Loading map dl_streets"));
        assert_eq!(d.last_match, Some(55555555));
        assert_eq!(d.session, Session::default());
        assert!(d.ring.len() >= 3);
    }

    #[test]
    fn library_folders_parse() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
        let libs = parse_library_folders(vdf);
        assert_eq!(
            libs,
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ]
        );
    }

    #[test]
    fn tail_reads_only_new_lines_and_handles_restart() {
        let dir = std::env::temp_dir().join(format!("dl_tail_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("console.log");
        std::fs::write(&p, "old line\n").unwrap();
        let mut t = LogTail::new(p.clone(), true);
        assert!(t.read_new().unwrap().is_empty());
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "new 1\r\nnew 2\npart").unwrap();
        drop(f);
        assert_eq!(t.read_new().unwrap(), vec!["new 1", "new 2"]);
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "ial\n").unwrap();
        drop(f);
        assert_eq!(t.read_new().unwrap(), vec!["partial"]);
        std::fs::write(&p, "fresh\n").unwrap(); // игра перезаписала лог
        assert_eq!(t.read_new().unwrap(), vec!["fresh"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
