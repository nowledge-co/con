unsafe extern "C" {
    fn con_install_window_cycle_shortcuts();
}

pub fn install_window_cycle_shortcuts() {
    unsafe { con_install_window_cycle_shortcuts() };
}
