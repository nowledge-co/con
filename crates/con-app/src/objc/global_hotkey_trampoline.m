#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#include <stdbool.h>
#include <stdint.h>

typedef void (*con_hotkey_callback_t)(void);

static EventHotKeyRef g_con_hotkey_ref = NULL;
static EventHandlerRef g_con_hotkey_handler = NULL;
static con_hotkey_callback_t g_con_hotkey_callback = NULL;
static id g_con_window_cycle_monitor = nil;
static NSMutableArray<NSNumber *> *g_con_window_cycle_order = nil;
static NSTimeInterval g_con_last_window_cycle_timestamp = 0;

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

static BOOL con_should_cycle_window(NSEvent *event, BOOL *reverse) {
    if (event.type != NSEventTypeKeyDown) {
        return NO;
    }

    NSEventModifierFlags flags = event.modifierFlags;
    BOOL command = (flags & NSEventModifierFlagCommand) != 0;
    BOOL control = (flags & NSEventModifierFlagControl) != 0;
    BOOL option = (flags & NSEventModifierFlagOption) != 0;
    BOOL shift = (flags & NSEventModifierFlagShift) != 0;

    if (!command || control || option) {
        return NO;
    }

    // Match the physical grave-accent key. AppKit's native Cmd-` shortcut is
    // window-management, not terminal input, and the embedded Ghostty NSView can
    // otherwise keep the event from reaching GPUI's keymap.
    if (event.keyCode != 0x32) {
        NSString *characters = event.charactersIgnoringModifiers ?: @"";
        if (![characters isEqualToString:@"`"] && ![characters isEqualToString:@"~"]) {
            return NO;
        }
    }

    if (reverse != NULL) {
        *reverse = shift;
    }
    return YES;
}

static NSArray<NSWindow *> *con_cycleable_windows(void) {
    NSMutableArray<NSWindow *> *windows = [NSMutableArray array];
    for (NSWindow *window in NSApp.orderedWindows) {
        if (!window.isVisible || window.isMiniaturized || !window.canBecomeKeyWindow) {
            continue;
        }
        [windows addObject:window];
    }
    return windows;
}

static NSWindow *con_find_cycleable_window(NSArray<NSWindow *> *windows, NSInteger window_number) {
    for (NSWindow *window in windows) {
        if (window.windowNumber == window_number) {
            return window;
        }
    }
    return nil;
}

static NSMutableArray<NSNumber *> *con_rebuild_window_cycle_order(NSArray<NSWindow *> *windows) {
    NSMutableArray<NSNumber *> *order = [NSMutableArray arrayWithCapacity:windows.count];
    for (NSWindow *window in windows) {
        [order addObject:@(window.windowNumber)];
    }
    return order;
}

static NSMutableArray<NSNumber *> *con_current_window_cycle_order(
    NSArray<NSWindow *> *windows,
    NSWindow *current,
    NSTimeInterval timestamp
) {
    BOOL expired = timestamp - g_con_last_window_cycle_timestamp > 2.0;
    NSNumber *current_number = @(current.windowNumber);
    if (g_con_window_cycle_order == nil ||
        expired ||
        ![g_con_window_cycle_order containsObject:current_number]) {
        return con_rebuild_window_cycle_order(windows);
    }

    NSMutableArray<NSNumber *> *order = [NSMutableArray arrayWithCapacity:windows.count];
    for (NSNumber *window_number in g_con_window_cycle_order) {
        if (con_find_cycleable_window(windows, window_number.integerValue) != nil) {
            [order addObject:window_number];
        }
    }
    for (NSWindow *window in windows) {
        NSNumber *window_number = @(window.windowNumber);
        if (![order containsObject:window_number]) {
            [order addObject:window_number];
        }
    }
    return order;
}

static void con_cycle_app_window_at_time(BOOL reverse, NSTimeInterval timestamp) {
    NSArray<NSWindow *> *windows = con_cycleable_windows();
    NSUInteger count = windows.count;
    if (count < 2) {
        return;
    }

    NSWindow *current = NSApp.keyWindow ?: NSApp.mainWindow ?: windows.firstObject;
    NSMutableArray<NSNumber *> *order = con_current_window_cycle_order(windows, current, timestamp);
    NSNumber *current_number = @(current.windowNumber);
    NSUInteger current_index = [order indexOfObject:current_number];
    if (current_index == NSNotFound) {
        order = con_rebuild_window_cycle_order(windows);
        current_index = [order indexOfObject:current_number];
        if (current_index == NSNotFound) {
            current_index = 0;
        }
    }

    count = order.count;
    NSUInteger target_index = reverse
        ? (current_index == 0 ? count - 1 : current_index - 1)
        : (current_index + 1) % count;

    NSWindow *target = con_find_cycleable_window(windows, order[target_index].integerValue);
    if (target == nil) {
        return;
    }

    g_con_window_cycle_order = order;
    g_con_last_window_cycle_timestamp = timestamp;

    [NSApp activateIgnoringOtherApps:YES];
    [target makeKeyAndOrderFront:nil];
}

void con_cycle_app_window(bool reverse) {
    con_cycle_app_window_at_time(reverse, [NSDate timeIntervalSinceReferenceDate]);
}

void con_install_window_cycle_shortcuts(void) {
    if (g_con_window_cycle_monitor != nil) {
        return;
    }

    g_con_window_cycle_monitor = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskKeyDown
                                                                       handler:^NSEvent *(NSEvent *event) {
        BOOL reverse = NO;
        if (con_should_cycle_window(event, &reverse)) {
            con_cycle_app_window_at_time(reverse, [NSDate timeIntervalSinceReferenceDate]);
            return nil;
        }
        return event;
    }];
}
