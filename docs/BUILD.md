# Deadlock Counterpick — приложение для Windows, Linux и Android

Rust + [Tauri 2](https://tauri.app). Интерфейс — тот же, что в веб-версии (`dist/index.html`),
а Rust добавляет нативные функции:

| Функция | Windows | Linux | Android |
|---|---|---|---|
| Все вкладки: профиль, лайв, драфт, предметы, тренировка | ✅ | ✅ | ✅ |
| Детектор игры: запуск Deadlock, герои и номер матча из лога, разбор сразу после матча | ✅ (экспериментально) | ✅ (Steam и Proton, экспериментально) | — |
| Горячие клавиши F6–F10 для таймеров прямо из игры | ✅ | ✅ на X11 (на Wayland могут не срабатывать) | — |
| Окно поверх игры (переключатель во вкладке «Лайв») | ✅ | ✅ | — |
| Уведомления таймеров | ✅ | ✅ | ✅ (приходят даже из фона) |
| Сохранение карточек в «Изображения/Deadlock Counterpick» | ✅ | ✅ (`~/Pictures`) | показ на экране |
| Голосовые подсказки | ✅ | — | — (звуковой сигнал и уведомления работают) |

Таймеры по клавишам: **F6** мид-босс, **F7** большой лагерь, **F8** малый лагерь, **F9** урна, **F10** свой.
Если какая-то клавиша занята другой программой, приложение просто её пропустит.

## Детектор игры (только ПК)

Приложение само находит папку Deadlock в библиотеках Steam и следит за игрой:
- видит, что Deadlock запущен;
- читает консольный лог игры и замечает героев и номер текущего матча;
- когда матч заканчивается, само подтягивает его из истории и предлагает разбор.

Это обычный текстовый файл, который игра пишет сама. Память игры не читается,
поэтому для аккаунта это безопасно.

**Включить один раз:** Steam → Библиотека → правый клик по Deadlock → Свойства → Общие →
Параметры запуска: впиши `-condebug` и перезапусти игру. После этого игра пишет лог в
`Deadlock\game\citadel\console.log`.

**Linux (Steam + Proton):** то же самое. Если в параметрах запуска уже есть `%command%`,
допиши флаг после него: `%command% -condebug`. Папку игры приложение ищет в `~/.steam/steam`,
`~/.local/share/Steam`, Steam из Flatpak и Snap, а также во всех библиотеках из `libraryfolders.vdf`.
Не нашло — впиши путь вручную во вкладке «Лайв», например `~/.local/share/Steam/steamapps/common/Deadlock`.

Детектор пока экспериментальный: Valve не публикует формат лога. После первого матча нажми во вкладке
«Лайв» кнопку **«Скопировать фрагмент лога»** и пришли текст: по нему распознавание составов и команд
можно сделать точным.

---

## Сборка с телефона (одним архивом)

Мобильный браузер не загружает папки, поэтому проект можно залить одним zip-файлом:
1. На github.com создай приватный репозиторий и поставь галочку **Add a README file**.
2. **Add file → Upload files** → выбери `deadlock-counterpick-rust.zip` целиком (не распаковывая) → **Commit changes**.
3. **Add file → Create new file** → в имени файла напиши `.github/workflows/build.yml`,
   вставь текст из файла `build.yml.txt` → **Commit changes**.
4. Вкладка **Actions**: сборка запустится сама. Готовые файлы скачиваются там же, в разделе **Artifacts**.

## Как получить готовые файлы (ничего не устанавливая)

Сборку делает GitHub бесплатно на своих серверах. Понадобится только аккаунт GitHub.

### Шаг 1. Создай репозиторий
1. Зайди на [github.com](https://github.com) и войди (или зарегистрируйся).
2. Справа вверху **+** → **New repository**.
3. Имя: `deadlock-counterpick`. Выбери **Private** (так ключ подписи и файлы видишь только ты).
4. Нажми **Create repository**.

### Шаг 2. Загрузи файлы
1. На странице нового репозитория нажми ссылку **uploading an existing file**.
2. Открой распакованную папку `deadlock-counterpick` в Проводнике, выдели **всё содержимое**
   (включая папку `.github`) и перетащи в окно браузера.
3. Внизу нажми **Commit changes**.

> Если папка `.github` не загрузилась перетаскиванием, нажми **Add file → Create new file**,
> в имени напиши `.github/workflows/build.yml` и вставь содержимое этого файла из архива.

### Шаг 3. Дождись сборки (~15–25 минут в первый раз)
1. Открой вкладку **Actions** в репозитории.
2. Там появится запуск **«Сборка приложений»**. Жёлтый кружок — идёт сборка, зелёная галочка — готово.
3. Открой запуск и прокрути вниз до раздела **Artifacts**:
   - **windows** — установщик `…_x64-setup.exe`, `.msi` и портативный `DeadlockCounterpick-portable.exe`;
   - **linux** — `DeadlockCounterpick-x86_64.AppImage`, `.deb` (Ubuntu, Debian, Mint) и `.rpm` (Fedora, openSUSE);
   - **android** — `DeadlockCounterpick.apk`;
   - **android-signing-key** — ключ подписи (только при первой сборке, см. шаг 5).

### Шаг 4. Установи
**Windows:** запусти `…_x64-setup.exe`. Windows может показать «Система Windows защитила ваш компьютер»
(у приложения нет платного сертификата) → **Подробнее** → **Выполнить в любом случае**.

**Linux:** самый простой вариант — AppImage, работает почти на любом дистрибутиве:
```bash
chmod +x DeadlockCounterpick-x86_64.AppImage
./DeadlockCounterpick-x86_64.AppImage
```
Или пакетом: `sudo apt install ./DeadlockCounterpick_amd64.deb` (Ubuntu, Debian, Mint, Pop!_OS),
`sudo dnf install ./DeadlockCounterpick.x86_64.rpm` (Fedora). После этого приложение появится в меню.
Если окно открывается белым или пустым: запусти с `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
Если AppImage не стартует на старом дистрибутиве: `sudo apt install libfuse2`.

**Android:** перекинь `DeadlockCounterpick.apk` на телефон (Telegram «Избранное», Google Диск, кабель),
открой файл и разреши установку из этого источника, когда телефон спросит.

### Шаг 5. Сохрани ключ подписи (один раз, 2 минуты)
Без этого шага каждая новая сборка APK будет подписана новым ключом, и для обновления
придётся удалять старую версию (данные переносятся через «Скачать бэкап» во вкладке «Тренировка»).

1. Скачай артефакт **android-signing-key** из первой сборки и распакуй: там два текстовых файла.
2. В репозитории: **Settings → Secrets and variables → Actions → New repository secret**.
3. Создай секрет `ANDROID_KEYSTORE_B64`, вставь содержимое файла `ANDROID_KEYSTORE_B64.txt`.
4. Создай секрет `ANDROID_KEYSTORE_PASSWORD`, вставь содержимое `ANDROID_KEYSTORE_PASSWORD.txt`.
5. Запусти сборку ещё раз (**Actions → Сборка приложений → Run workflow**) и установи **эту** APK.
   Все следующие версии будут ставиться поверх неё.

### Обновления
Заменил файлы в репозитории → GitHub сам соберёт новую версию.
Чтобы получить красивую страницу релиза со всеми файлами: **Releases → Draft a new release →**
тег `v1.0.1` → **Publish**. Сборка запустится сама и прикрепит файлы к релизу.

---

## Сборка на своём ПК (по желанию)

**Windows:**
1. Установи [Rust](https://rustup.rs) (вариант по умолчанию, вместе с Visual Studio Build Tools) и [Node.js LTS](https://nodejs.org).
2. В папке проекта: `npm install`, затем `npx tauri dev` (запуск) или `npx tauri build` (установщик в `src-tauri/target/release/bundle`).

**Linux:** установи [Rust](https://rustup.rs), Node.js LTS и системные библиотеки
(Ubuntu/Debian: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev build-essential`;
другие дистрибутивы: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/#linux)). Дальше те же `npm install` и `npx tauri build`.

**Android:** нужен ещё Android Studio с SDK и NDK. Пошагово: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/#android).
Затем `npx tauri android init` и `npx tauri android build --apk`.

## Структура
```
dist/index.html                интерфейс (общий с веб-версией)
src-tauri/src/lib.rs           Rust: горячие клавиши, окно поверх, сохранение карточек, уведомления
src-tauri/src/game.rs          Rust: детектор игры (поиск папки, процесс, чтение лога) с тестами
src-tauri/tauri.conf.json      настройки приложения и установщика
src-tauri/capabilities/        права интерфейса
src-tauri/icons/               иконки
.github/workflows/build.yml    автосборка Windows, Linux и Android
```
