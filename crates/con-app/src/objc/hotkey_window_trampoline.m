#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <stdbool.h>

static NSRect con_hotkey_window_hidden_frame(NSWindow *window) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    NSRect frame = window.frame;
    frame.origin.x = NSMidX(visible) - (frame.size.width / 2.0);
    frame.origin.y = NSMaxY(visible);
    return frame;
}

static NSRect con_hotkey_window_visible_frame(NSWindow *window) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    NSRect frame = window.frame;
    frame.origin.x = NSMidX(visible) - (frame.size.width / 2.0);
    frame.origin.y = NSMaxY(visible) - frame.size.height;
    return frame;
}

void con_hotkey_window_configure(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    window.styleMask = NSWindowStyleMaskBorderless;
    window.collectionBehavior =
        NSWindowCollectionBehaviorMoveToActiveSpace |
        NSWindowCollectionBehaviorTransient;
    window.opaque = NO;
    window.hasShadow = YES;
    window.hidesOnDeactivate = NO;
    window.releasedWhenClosed = NO;
    window.movable = NO;
    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
    [window setFrame:con_hotkey_window_hidden_frame(window) display:NO];
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

void con_hotkey_window_slide_in(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    [window setFrame:con_hotkey_window_hidden_frame(window) display:NO];
    [window orderFront:nil];

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = 0.22;
        context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
        [[window animator] setFrame:con_hotkey_window_visible_frame(window) display:YES];
    } completionHandler:nil];
}

void con_hotkey_window_slide_out(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = 0.18;
        context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
        [[window animator] setFrame:con_hotkey_window_hidden_frame(window) display:YES];
    } completionHandler:^{
        [window orderOut:nil];
    }];
}
