#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <math.h>
#include <objc/runtime.h>
#include <stdbool.h>
#include <stdint.h>

extern void ghostty_surface_set_content_scale(void *surface, double x, double y);
extern void ghostty_surface_set_display_id(void *surface, uint32_t display_id);
extern void ghostty_surface_set_size(void *surface, uint32_t width, uint32_t height);

static char kConGhosttySurfaceBackingObserverKey;

static double con_valid_scale(double scale, double fallback) {
    if (!isfinite(scale) || scale <= 0.0) {
        return fallback > 0.0 ? fallback : 1.0;
    }
    return scale;
}

static uint32_t con_ghostty_display_id_for_window(NSWindow *window) {
    NSScreen *screen = window.screen;
    if (screen == nil) {
        return 0;
    }

    NSNumber *number = screen.deviceDescription[@"NSScreenNumber"];
    return number == nil ? 0 : number.unsignedIntValue;
}

bool con_ghostty_surface_sync_backing(
    void *view_ptr,
    void *surface_ptr,
    double logical_width,
    double logical_height,
    double fallback_scale,
    double *out_scale_x,
    double *out_scale_y,
    uint32_t *out_width,
    uint32_t *out_height
) {
    NSView *view = (__bridge NSView *)view_ptr;
    if (view == nil || surface_ptr == NULL) {
        return false;
    }

    fallback_scale = con_valid_scale(fallback_scale, 1.0);

    NSWindow *window = view.window;
    double layer_scale = fallback_scale;
    uint32_t display_id = 0;
    if (window != nil) {
        layer_scale = con_valid_scale(window.backingScaleFactor, fallback_scale);
        display_id = con_ghostty_display_id_for_window(window);
    }

    CALayer *layer = view.layer;
    if (layer != nil) {
        [CATransaction begin];
        [CATransaction setDisableActions:YES];
        layer.contentsScale = layer_scale;
        [CATransaction commit];
    }

    NSRect frame = view.frame;
    NSRect backing_frame = [view convertRectToBacking:frame];
    double scale_x = frame.size.width > 0.0
        ? backing_frame.size.width / frame.size.width
        : fallback_scale;
    double scale_y = frame.size.height > 0.0
        ? backing_frame.size.height / frame.size.height
        : fallback_scale;
    scale_x = con_valid_scale(scale_x, fallback_scale);
    scale_y = con_valid_scale(scale_y, fallback_scale);

    NSSize logical_size = NSMakeSize(fmax(logical_width, 1.0), fmax(logical_height, 1.0));
    NSSize backing_size = [view convertSizeToBacking:logical_size];
    uint32_t width = (uint32_t)llround(fmax(backing_size.width, 1.0));
    uint32_t height = (uint32_t)llround(fmax(backing_size.height, 1.0));

    if (display_id != 0) {
        ghostty_surface_set_display_id(surface_ptr, display_id);
    }
    ghostty_surface_set_content_scale(surface_ptr, scale_x, scale_y);
    ghostty_surface_set_size(surface_ptr, width, height);

    if (out_scale_x != NULL) {
        *out_scale_x = scale_x;
    }
    if (out_scale_y != NULL) {
        *out_scale_y = scale_y;
    }
    if (out_width != NULL) {
        *out_width = width;
    }
    if (out_height != NULL) {
        *out_height = height;
    }

    return true;
}

@interface ConGhosttySurfaceBackingObserver : NSObject
@property(nonatomic, weak) NSView *view;
@property(nonatomic, assign) void *surface;
@property(nonatomic, weak) NSWindow *window;
@property(nonatomic, strong) id screenObserver;
@property(nonatomic, strong) id backingObserver;
- (void)installForView:(NSView *)view surface:(void *)surface;
- (void)invalidate;
- (void)sync;
@end

@implementation ConGhosttySurfaceBackingObserver

- (void)dealloc {
    [self invalidate];
}

- (void)installForView:(NSView *)view surface:(void *)surface {
    NSWindow *window = view.window;
    if (_window == window && _surface == surface && _screenObserver != nil && _backingObserver != nil) {
        [self sync];
        return;
    }

    [self invalidate];
    _view = view;
    _surface = surface;
    _window = window;

    if (window == nil || surface == NULL) {
        return;
    }

    __weak ConGhosttySurfaceBackingObserver *weakSelf = self;
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    _screenObserver = [center addObserverForName:NSWindowDidChangeScreenNotification
                                          object:window
                                           queue:NSOperationQueue.mainQueue
                                      usingBlock:^(__unused NSNotification *notification) {
        ConGhosttySurfaceBackingObserver *observer = weakSelf;
        [observer sync];
        dispatch_async(dispatch_get_main_queue(), ^{
            [observer sync];
        });
    }];
    _backingObserver = [center addObserverForName:NSWindowDidChangeBackingPropertiesNotification
                                           object:window
                                            queue:NSOperationQueue.mainQueue
                                       usingBlock:^(__unused NSNotification *notification) {
        [weakSelf sync];
    }];

    [self sync];
}

- (void)invalidate {
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    if (_screenObserver != nil) {
        [center removeObserver:_screenObserver];
        _screenObserver = nil;
    }
    if (_backingObserver != nil) {
        [center removeObserver:_backingObserver];
        _backingObserver = nil;
    }
    _window = nil;
    _surface = NULL;
}

- (void)sync {
    NSView *view = _view;
    if (view == nil || _surface == NULL) {
        return;
    }

    NSSize size = view.bounds.size;
    double fallback_scale = view.window == nil ? 1.0 : view.window.backingScaleFactor;
    con_ghostty_surface_sync_backing(
        (__bridge void *)view,
        _surface,
        size.width,
        size.height,
        fallback_scale,
        NULL,
        NULL,
        NULL,
        NULL
    );
}

@end

void con_ghostty_surface_install_backing_observer(void *view_ptr, void *surface_ptr) {
    NSView *view = (__bridge NSView *)view_ptr;
    if (view == nil || surface_ptr == NULL) {
        return;
    }

    ConGhosttySurfaceBackingObserver *observer =
        objc_getAssociatedObject(view, &kConGhosttySurfaceBackingObserverKey);
    if (observer == nil) {
        observer = [[ConGhosttySurfaceBackingObserver alloc] init];
        objc_setAssociatedObject(
            view,
            &kConGhosttySurfaceBackingObserverKey,
            observer,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC
        );
    }

    [observer installForView:view surface:surface_ptr];
}

void con_ghostty_surface_remove_backing_observer(void *view_ptr) {
    NSView *view = (__bridge NSView *)view_ptr;
    if (view == nil) {
        return;
    }

    ConGhosttySurfaceBackingObserver *observer =
        objc_getAssociatedObject(view, &kConGhosttySurfaceBackingObserverKey);
    [observer invalidate];
    objc_setAssociatedObject(
        view,
        &kConGhosttySurfaceBackingObserverKey,
        nil,
        OBJC_ASSOCIATION_RETAIN_NONATOMIC
    );
}
