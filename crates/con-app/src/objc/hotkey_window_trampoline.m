#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <dispatch/dispatch.h>

static const CGFloat CON_HOTKEY_WINDOW_MIN_HEIGHT = 280.0;

static NSRect con_hotkey_window_frame(NSWindow *window, bool visible) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visibleFrame = screen.visibleFrame;
    NSRect frame = window.frame;
    CGFloat clampedHeight = fmin(fmax(NSHeight(frame), CON_HOTKEY_WINDOW_MIN_HEIGHT), visibleFrame.size.height);

    frame.origin.x = visibleFrame.origin.x;
    frame.size.width = visibleFrame.size.width;
    frame.size.height = clampedHeight;
    frame.origin.y = visible ? (NSMaxY(visibleFrame) - clampedHeight) : NSMaxY(visibleFrame);
    return frame;
}

static void con_hotkey_window_apply_configuration(NSWindow *window, bool always_on_top) {
    window.styleMask = NSWindowStyleMaskBorderless | NSWindowStyleMaskResizable;
    window.collectionBehavior = NSWindowCollectionBehaviorMoveToActiveSpace |
                                NSWindowCollectionBehaviorTransient;
    window.opaque = NO;
    window.hasShadow = YES;
    window.hidesOnDeactivate = NO;
    window.releasedWhenClosed = NO;
    window.movable = NO;
    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
    window.contentMinSize = NSMakeSize(320.0, CON_HOTKEY_WINDOW_MIN_HEIGHT);
    [window setFrame:con_hotkey_window_frame(window, false) display:NO];
    [window orderOut:nil];
}

void con_hotkey_window_configure(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    if ([NSThread isMainThread]) {
        con_hotkey_window_apply_configuration(window, always_on_top);
        return;
    }

    dispatch_sync(dispatch_get_main_queue(), ^{
        con_hotkey_window_apply_configuration(window, always_on_top);
    });
}

void con_hotkey_window_set_level(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
}

void *con_hotkey_window_window_from_view(void *view_ptr) {
    NSView *view = (__bridge NSView *)view_ptr;
    if (view == nil) {
        return NULL;
    }

    return (__bridge void *)view.window;
}

int32_t con_hotkey_window_frontmost_app_pid(void) {
    NSRunningApplication *app = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (app == nil) {
        return 0;
    }

    return (int32_t)app.processIdentifier;
}

bool con_hotkey_window_activate_app(int32_t pid) {
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

void con_hotkey_window_slide_in(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [window setFrame:con_hotkey_window_frame(window, false) display:NO];
        [NSApp activateIgnoringOtherApps:YES];
        [window makeKeyAndOrderFront:nil];

        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
            context.duration = 0.18;
            context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
            [[window animator] setFrame:con_hotkey_window_frame(window, true) display:YES];
        } completionHandler:nil];
    });
}

void con_hotkey_window_slide_out(void *window_ptr, int32_t return_pid) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
            context.duration = 0.14;
            context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
            [[window animator] setFrame:con_hotkey_window_frame(window, false) display:YES];
        } completionHandler:^{
            [window orderOut:nil];
            if (return_pid > 0) {
                con_hotkey_window_activate_app(return_pid);
            }
        }];
    });
}
