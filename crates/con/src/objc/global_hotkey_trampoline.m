#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#include <stdbool.h>
#include <stdint.h>

typedef void (*con_hotkey_callback_t)(void);

static EventHotKeyRef g_con_hotkey_ref = NULL;
static EventHandlerRef g_con_hotkey_handler = NULL;
static con_hotkey_callback_t g_con_hotkey_callback = NULL;

static OSStatus con_hotkey_handler(EventHandlerCallRef nextHandler, EventRef event, void *userData) {
    (void)nextHandler;
    (void)userData;
    if (GetEventClass(event) == kEventClassKeyboard &&
        GetEventKind(event) == kEventHotKeyPressed &&
        g_con_hotkey_callback != NULL) {
        g_con_hotkey_callback();
    }
    return noErr;
}

bool con_register_global_hotkey(
    uint32_t key_code,
    bool shift,
    bool control,
    bool alt,
    bool command,
    con_hotkey_callback_t callback
) {
    if (g_con_hotkey_ref != NULL) {
        UnregisterEventHotKey(g_con_hotkey_ref);
        g_con_hotkey_ref = NULL;
    }

    if (g_con_hotkey_handler == NULL) {
        EventTypeSpec spec = {
            .eventClass = kEventClassKeyboard,
            .eventKind = kEventHotKeyPressed,
        };
        InstallApplicationEventHandler(
            NewEventHandlerUPP(con_hotkey_handler),
            1,
            &spec,
            NULL,
            &g_con_hotkey_handler
        );
    }

    g_con_hotkey_callback = callback;

    UInt32 modifiers = 0;
    if (shift) {
        modifiers |= shiftKey;
    }
    if (control) {
        modifiers |= controlKey;
    }
    if (alt) {
        modifiers |= optionKey;
    }
    if (command) {
        modifiers |= cmdKey;
    }

    EventHotKeyID hotkey_id = {
        .signature = 'conh',
        .id = 1,
    };

    OSStatus status = RegisterEventHotKey(
        key_code,
        modifiers,
        hotkey_id,
        GetApplicationEventTarget(),
        0,
        &g_con_hotkey_ref
    );

    return status == noErr;
}

void con_unregister_global_hotkey(void) {
    if (g_con_hotkey_ref != NULL) {
        UnregisterEventHotKey(g_con_hotkey_ref);
        g_con_hotkey_ref = NULL;
    }
    g_con_hotkey_callback = NULL;
}

bool con_app_is_active(void) {
    return NSApp != nil && [NSApp isActive];
}
