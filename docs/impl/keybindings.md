# Keybinding Scopes

Status: Implemented

`crates/con-app/src/main.rs` defines keybindings through a small structured
table instead of scattered `KeyBinding::new(...)` calls.

## Model

`BindingSpec` records:

- key string,
- action `TypeId`,
- logical `BindingScope`,
- conversion function to a GPUI `KeyBinding`.

`BindingScope` maps to GPUI scopes:

| Scope | GPUI scope | Use |
|---|---|---|
| `Global` | `None` | App-level shortcuts such as tabs, panes, command palette, left sidebar. |
| `Input` | `Some("Input")` | Explicit overrides for text inputs only when proven necessary. |
| `EditorView` | `Some("EditorView")` | Editor text-editing and cursor movement keys. |

## Rules

- App shortcuts are global by default. GPUI global bindings continue to apply
  across focused contexts unless a more specific binding consumes the key.
- Editor text-editing shortcuts stay editor-only. Enter, arrows, Backspace,
  Delete, `Ctrl+A`, and `Ctrl+E` must not leak into terminal or input scopes.
- Multi-context bindings require an explicit helper such as
  `push_app_override`; they should not be copy-pasted as several direct
  `KeyBinding::new(...)` calls.
- Terminal-reserved keys are never bound globally for editor behavior.

## Helpers

```text
push_global   -> [Global]
push_editor   -> [EditorView]
push_app_override -> [Global, Input, EditorView]
binding_specs -> all configured/default binding specs
bind_specs    -> GPUI KeyBinding registration
```

## Regression Coverage

The unit tests in `main.rs` assert:

- app shortcuts are single global bindings,
- editor shortcuts stay scoped to `EditorView`,
- Enter is only bound to `EditorInsertNewline` in editor scope,
- explicit multi-context overrides remain representable,
- generated specs do not duplicate the same key/action/scope tuple.
