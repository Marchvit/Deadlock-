// В релизной сборке на Windows не показывать консольное окно
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Linux: WebKitGTK с аппаратным DMA-BUF часто показывает белое окно
    // на видеокартах Nvidia и под Wayland. Отключаем, если пользователь не задал сам.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    deadlock_counterpick_lib::run()
}
