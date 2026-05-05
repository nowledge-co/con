#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <dispatch/dispatch.h>
#include <objc/runtime.h>
#include <unistd.h>

static const CGFloat CON_QUICK_TERMINAL_MIN_HEIGHT = 280.0;

extern void con_quick_terminal_handle_resign_key(void);
extern void con_quick_terminal_remember_active_app(int32_t pid);

static char kResignKeyObserverKey;
static id gActiveAppObserver = nil;

void con_quick_terminal_init(void) {
    if (gActiveAppObserver != nil) {
        return;
    }

    gActiveAppObserver = [NSWorkspace.sharedWorkspace.notificationCenter
        addObserverForName:NSWorkspaceDidActivateApplicationNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification *notification) {
        NSRunningApplication *app = notification.userInfo[NSWorkspaceApplicationKey];
        if (app != nil) {
            con_quick_terminal_remember_active_app((int32_t)app.processIdentifier);
        }
    }];
}

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

static void con_quick_terminal_apply_configuration(NSWindow *window) {
    // Borderless removes the visible chrome; the resizable bit keeps the
    // AppKit edge-resize behavior for the bottom edge and preserves live
    // height changes across show/hide animations.
    window.styleMask = NSWindowStyleMaskBorderless | NSWindowStyleMaskResizable;
    window.collectionBehavior = NSWindowCollectionBehaviorMoveToActiveSpace |
                                NSWindowCollectionBehaviorTransient;
    window.opaque = NO;
    window.hasShadow = YES;
    window.hidesOnDeactivate = NO;
    window.releasedWhenClosed = NO;
    window.movable = NO;
    window.level = NSNormalWindowLevel;
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
                     queue:NSOperationQueue.mainQueue
                usingBlock:^(__unused NSNotification *note) {
                    con_quick_terminal_handle_resign_key();
                }];
    objc_setAssociatedObject(window, &kResignKeyObserverKey, observer,
                             OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

static void con_quick_terminal_remove_resign_observer(NSWindow *window) {
    id observer = objc_getAssociatedObject(window, &kResignKeyObserverKey);
    if (observer) {
        [[NSNotificationCenter defaultCenter] removeObserver:observer];
        objc_setAssociatedObject(window, &kResignKeyObserverKey, nil,
                                 OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
}

void con_quick_terminal_configure(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        con_quick_terminal_apply_configuration(window);
    });
}

void con_quick_terminal_prepare_destroy(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    if ([NSThread isMainThread]) {
        con_quick_terminal_remove_resign_observer(window);
    } else {
        dispatch_sync(dispatch_get_main_queue(), ^{
            con_quick_terminal_remove_resign_observer(window);
        });
    }
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

bool con_quick_terminal_active_display_visible_frame(double *x, double *y, double *width, double *height) {
    NSPoint mouseLocation = NSEvent.mouseLocation;
    NSScreen *target = nil;
    for (NSScreen *screen in NSScreen.screens) {
        if (NSPointInRect(mouseLocation, screen.frame)) {
            target = screen;
            break;
        }
    }
    if (target == nil) {
        target = NSScreen.mainScreen;
    }
    if (target == nil) {
        return false;
    }

    NSRect frame = target.visibleFrame;
    if (x != NULL) {
        *x = frame.origin.x;
    }
    if (y != NULL) {
        *y = frame.origin.y;
    }
    if (width != NULL) {
        *width = frame.size.width;
    }
    if (height != NULL) {
        *height = frame.size.height;
    }
    return true;
}

bool con_quick_terminal_is_main_thread(void) {
    return [NSThread isMainThread];
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

    // Explicitly activate the target app. `yieldActivationToApplication:` is
    // weaker here: when the quick terminal orders out, AppKit may make con's
    // main window key before the yielded activation wins, so focus appears to
    // fall back to con. `activateWithOptions` matches the older behavior and
    // reliably restores the recorded app, which may itself be con.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    return [app activateWithOptions:NSApplicationActivateIgnoringOtherApps];
#pragma clang diagnostic pop
}

void con_quick_terminal_slide_in(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [window setFrame:con_quick_terminal_frame(window, false) display:NO];
        if (@available(macOS 14.0, *)) {
            [NSApp activate];
        } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            [NSApp activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
        }
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
            if (return_pid > 0 && return_pid != (int32_t)getpid()) {
                con_quick_terminal_activate_app(return_pid);
            }
            [window orderOut:nil];
        }];
    });
}
