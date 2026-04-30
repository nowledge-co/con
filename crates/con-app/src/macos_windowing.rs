unsafe extern "C" {
    fn con_install_window_cycle_shortcuts();
    fn con_cycle_app_window(reverse: bool);
}

pub fn install_window_cycle_shortcuts() {
    unsafe { con_install_window_cycle_shortcuts() };
}

pub fn cycle_app_window(reverse: bool) {
    unsafe { con_cycle_app_window(reverse) };
}
