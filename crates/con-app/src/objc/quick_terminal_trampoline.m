#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <dispatch/dispatch.h>
#include <objc/runtime.h>

static const CGFloat CON_QUICK_TERMINAL_MIN_HEIGHT = 280.0;

extern void con_quick_terminal_handle_resign_key(void);

static char kResignKeyObserverKey;

static NSRect con_quick_terminal_frame(NSWindow *window, bool visible) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visibleFrame = screen.visibleFrame;
    CGFloat halfHeight = visibleFrame.size.height / 2.0;
    // Use the window's current height if GPUI has already applied it,
    // otherwise fall back to half the screen height (the initial default).
    CGFloat height = NSHeight(window.frame);
    if (height <= 0.0) {
        height = halfHeight;
    }
    CGFloat clampedHeight = fmin(fmax(height, CON_QUICK_TERMINAL_MIN_HEIGHT), visibleFrame.size.height);

    NSRect frame;
    frame.origin.x = visibleFrame.origin.x;
    frame.size.width = visibleFrame.size.width;
    frame.size.height = clampedHeight;
    frame.origin.y = visible ? (NSMaxY(visibleFrame) - clampedHeight) : NSMaxY(visibleFrame);
    return frame;
}

static void con_quick_terminal_apply_configuration(NSWindow *window, bool always_on_top) {
    window.styleMask = NSWindowStyleMaskBorderless | NSWindowStyleMaskResizable;
    window.collectionBehavior = NSWindowCollectionBehaviorMoveToActiveSpace |
                                NSWindowCollectionBehaviorTransient;
    window.opaque = NO;
    window.hasShadow = YES;
    window.hidesOnDeactivate = NO;
    window.releasedWhenClosed = NO;
    window.movable = NO;
    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
    window.contentMinSize = NSMakeSize(320.0, CON_QUICK_TERMINAL_MIN_HEIGHT);
    [window setFrame:con_quick_terminal_frame(window, false) display:NO];
    [window orderOut:nil];

    // Auto-hide when the window loses focus (user clicks elsewhere).
    id oldObserver = objc_getAssociatedObject(window, &kResignKeyObserverKey);
    if (oldObserver) {
        [[NSNotificationCenter defaultCenter] removeObserver:oldObserver];
    }
    id observer = [[NSNotificationCenter defaultCenter]
        addObserverForName:NSWindowDidResignKeyNotification
                    object:window
                     queue:nil
                usingBlock:^(__unused NSNotification *note) {
                    con_quick_terminal_handle_resign_key();
                }];
    objc_setAssociatedObject(window, &kResignKeyObserverKey, observer,
                             OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

void con_quick_terminal_configure(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        con_quick_terminal_apply_configuration(window, always_on_top);
    });
}

void con_quick_terminal_set_level(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
    });
}

void *con_quick_terminal_window_from_view(void *view_ptr) {
    NSView *view = (__bridge NSView *)view_ptr;
    if (view == nil) {
        return NULL;
    }

    return (__bridge void *)view.window;
}

int32_t con_quick_terminal_frontmost_app_pid(void) {
    NSRunningApplication *app = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (app == nil) {
        return 0;
    }

    return (int32_t)app.processIdentifier;
}

bool con_quick_terminal_activate_app(int32_t pid) {
    if (pid <= 0) {
        return false;
    }

    NSRunningApplication *app =
        [NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)pid];
    if (app == nil) {
        return false;
    }

    return [app activateWithOptions:NSApplicationActivateIgnoringOtherApps];
}

void con_quick_terminal_slide_in(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [window setFrame:con_quick_terminal_frame(window, false) display:NO];
        [NSApp activateIgnoringOtherApps:YES];
        [window makeKeyAndOrderFront:nil];

        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
            context.duration = 0.18;
            context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
            [[window animator] setFrame:con_quick_terminal_frame(window, true) display:YES];
        } completionHandler:nil];
    });
}

void con_quick_terminal_slide_out(void *window_ptr, int32_t return_pid) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
            context.duration = 0.14;
            context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
            [[window animator] setFrame:con_quick_terminal_frame(window, false) display:YES];
        } completionHandler:^{
            [window orderOut:nil];
            if (return_pid > 0) {
                con_quick_terminal_activate_app(return_pid);
            }
        }];
    });
}
