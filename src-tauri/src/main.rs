// В релизной сборке на Windows не показывать консольное окно
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    deadlock_counterpick_lib::run()
}
