#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_void};

pub type ghostty_app_t = *mut c_void;
pub type ghostty_surface_t = *mut c_void;
pub type ghostty_config_t = *mut c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ghostty_platform_e {
    GHOSTTY_PLATFORM_INVALID = 0,
    GHOSTTY_PLATFORM_MACOS = 1,
    GHOSTTY_PLATFORM_IOS = 2,
    GHOSTTY_PLATFORM_HEADLESS = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ghostty_color_scheme_e {
    GHOSTTY_COLOR_SCHEME_LIGHT = 0,
    GHOSTTY_COLOR_SCHEME_DARK = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ghostty_input_action_e {
    GHOSTTY_ACTION_RELEASE = 0,
    GHOSTTY_ACTION_PRESS = 1,
    GHOSTTY_ACTION_REPEAT = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ghostty_mouse_button_e {
    GHOSTTY_MOUSE_NONE = 0,
    GHOSTTY_MOUSE_LEFT = 1,
    GHOSTTY_MOUSE_RIGHT = 2,
    GHOSTTY_MOUSE_MIDDLE = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ghostty_surface_context_e {
    GHOSTTY_SURFACE_CONTEXT_WINDOW = 0,
    GHOSTTY_SURFACE_CONTEXT_TAB = 1,
    GHOSTTY_SURFACE_CONTEXT_SPLIT = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ghostty_platform_headless_s {
    pub scale_factor: c_double,
}

#[repr(C)]
pub union ghostty_platform_u {
    pub macos: ghostty_platform_macos_s,
    pub ios: ghostty_platform_ios_s,
    pub headless: ghostty_platform_headless_s,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ghostty_platform_macos_s {
    pub nsview: *mut c_void,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ghostty_platform_ios_s {
    pub uiview: *mut c_void,
}

#[repr(C)]
pub struct ghostty_surface_config_s {
    pub platform_tag: c_int,
    pub platform: ghostty_platform_u,
    pub userdata: *mut c_void,
    pub scale_factor: c_double,
    pub font_size: f32,
    pub working_directory: *const c_char,
    pub command: *const c_char,
    pub env_vars: *mut ghostty_env_var_s,
    pub env_var_count: usize,
    pub initial_input: *const c_char,
    pub wait_after_command: bool,
    pub context: ghostty_surface_context_e,
}

#[repr(C)]
pub struct ghostty_env_var_s {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
pub struct ghostty_surface_size_s {
    pub columns: u16,
    pub rows: u16,
    pub width_px: u32,
    pub height_px: u32,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

// App callbacks — these are function pointers passed during app creation.
// We define the types but the actual callback setup is complex;
// for now we expose only the essential surface lifecycle APIs.

unsafe extern "C" {
    // Init / shutdown
    pub fn ghostty_init(argc: usize, argv: *mut *mut c_char) -> c_int;

    // Config
    pub fn ghostty_config_new() -> ghostty_config_t;
    pub fn ghostty_config_free(config: ghostty_config_t);
    pub fn ghostty_config_load(config: ghostty_config_t);
    pub fn ghostty_config_finalize(config: ghostty_config_t);
    pub fn ghostty_config_set(
        config: ghostty_config_t,
        key: *const c_char,
        value: *const c_char,
    ) -> bool;

    // Surface config
    pub fn ghostty_surface_config_new() -> ghostty_surface_config_s;

    // Surface lifecycle (app-managed)
    pub fn ghostty_surface_free(surface: ghostty_surface_t);
    pub fn ghostty_surface_userdata(surface: ghostty_surface_t) -> *mut c_void;

    // Surface rendering
    pub fn ghostty_surface_draw(surface: ghostty_surface_t);
    pub fn ghostty_surface_refresh(surface: ghostty_surface_t);

    // Surface size
    pub fn ghostty_surface_set_size(surface: ghostty_surface_t, w: u32, h: u32);
    pub fn ghostty_surface_size(surface: ghostty_surface_t) -> ghostty_surface_size_s;
    pub fn ghostty_surface_set_content_scale(
        surface: ghostty_surface_t,
        x: c_double,
        y: c_double,
    );

    // Surface input
    pub fn ghostty_surface_set_focus(surface: ghostty_surface_t, focused: bool);
    pub fn ghostty_surface_text(surface: ghostty_surface_t, text: *const c_char);
    pub fn ghostty_surface_mouse_button(
        surface: ghostty_surface_t,
        action: ghostty_input_action_e,
        button: ghostty_mouse_button_e,
        mods: c_int,
    );
    pub fn ghostty_surface_mouse_pos(surface: ghostty_surface_t, x: c_double, y: c_double);
    pub fn ghostty_surface_mouse_scroll(
        surface: ghostty_surface_t,
        x: c_double,
        y: c_double,
        mods: c_int,
    );

    // Surface state
    pub fn ghostty_surface_set_color_scheme(
        surface: ghostty_surface_t,
        scheme: ghostty_color_scheme_e,
    );
    pub fn ghostty_surface_process_exited(surface: ghostty_surface_t) -> bool;
    pub fn ghostty_surface_needs_confirm_quit(surface: ghostty_surface_t) -> bool;

    // Agent state APIs
    pub fn ghostty_surface_get_title(surface: ghostty_surface_t) -> *const c_char;
    pub fn ghostty_surface_get_pwd(surface: ghostty_surface_t) -> *const c_char;
    pub fn ghostty_surface_get_pwd_len(surface: ghostty_surface_t) -> u32;
    pub fn ghostty_surface_is_alt_screen(surface: ghostty_surface_t) -> bool;
    pub fn ghostty_surface_cursor_pos(surface: ghostty_surface_t, col: *mut u16, row: *mut u16);
    pub fn ghostty_surface_screen_text(
        surface: ghostty_surface_t,
        buf: *mut c_char,
        buf_len: u32,
    ) -> u32;

    // IOSurface (macOS only)
    #[cfg(target_os = "macos")]
    pub fn ghostty_surface_iosurface(surface: ghostty_surface_t) -> *mut c_void;
}
