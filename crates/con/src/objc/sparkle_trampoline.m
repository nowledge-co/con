// ObjC exception trampoline for Sparkle initialization.
//
// Rust's catch_unwind cannot catch ObjC exceptions (__rust_foreign_exception).
// This file provides @try/@catch wrappers so the Rust side never sees an
// uncatchable exception from Sparkle.
//
// Sparkle classes are loaded dynamically at runtime, so we use the ObjC
// runtime API (objc_msgSend) rather than compiled method calls.

#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <objc/runtime.h>

/// Initialize SPUStandardUpdaterController with @try/@catch.
///
/// Returns the initialized controller, or NULL if an ObjC exception was thrown.
/// Uses initForStartingUpdater:NO so the caller controls when checking begins.
void *con_sparkle_init_controller(void) {
    @try {
        Class cls = NSClassFromString(@"SPUStandardUpdaterController");
        if (!cls) return NULL;

        id alloc = [cls alloc];
        if (!alloc) return NULL;

        // initForStartingUpdater:(BOOL)startingUpdater
        //   updaterDelegate:(id)updaterDelegate
        //   userDriverDelegate:(id)userDriverDelegate
        SEL initSel = NSSelectorFromString(
            @"initForStartingUpdater:updaterDelegate:userDriverDelegate:");

        // Use objc_msgSend typed cast for the 3-arg init
        typedef id (*InitIMP)(id, SEL, BOOL, id, id);
        InitIMP initFunc = (InitIMP)objc_msgSend;
        id controller = initFunc(alloc, initSel, NO, nil, nil);

        if (!controller) return NULL;
        return (__bridge_retained void *)controller;
    } @catch (NSException *exception) {
        NSLog(@"[con-updater] ObjC exception during Sparkle init: %@ — %@",
              exception.name, exception.reason);
        return NULL;
    }
}

/// Call -[SPUUpdater startUpdater:] with @try/@catch.
///
/// Returns 1 on success, 0 on failure or exception.
int con_sparkle_start_updater(void *controller) {
    @try {
        id ctrl = (__bridge id)controller;

        // Get the SPUUpdater from the controller
        SEL updaterSel = NSSelectorFromString(@"updater");
        id updater = ((id (*)(id, SEL))objc_msgSend)(ctrl, updaterSel);
        if (!updater) return 0;

        // -[SPUUpdater startUpdater:] returns BOOL, takes NSError**
        SEL startSel = NSSelectorFromString(@"startUpdater:");
        NSError *error = nil;
        typedef BOOL (*StartIMP)(id, SEL, NSError **);
        StartIMP startFunc = (StartIMP)objc_msgSend;
        BOOL ok = startFunc(updater, startSel, &error);
        if (!ok) {
            NSLog(@"[con-updater] startUpdater failed: %@", error);
            return 0;
        }
        return 1;
    } @catch (NSException *exception) {
        NSLog(@"[con-updater] ObjC exception in startUpdater: %@ — %@",
              exception.name, exception.reason);
        return 0;
    }
}

/// Call -[SPUUpdater checkForUpdates] with @try/@catch.
void con_sparkle_check_for_updates(void *controller) {
    @try {
        id ctrl = (__bridge id)controller;

        SEL updaterSel = NSSelectorFromString(@"updater");
        id updater = ((id (*)(id, SEL))objc_msgSend)(ctrl, updaterSel);
        if (!updater) return;

        SEL checkSel = NSSelectorFromString(@"checkForUpdates");
        ((void (*)(id, SEL))objc_msgSend)(updater, checkSel);
    } @catch (NSException *exception) {
        NSLog(@"[con-updater] ObjC exception in checkForUpdates: %@ — %@",
              exception.name, exception.reason);
    }
}
