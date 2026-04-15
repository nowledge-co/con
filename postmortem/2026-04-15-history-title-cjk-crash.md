# History title truncation crashed on CJK text

## What happened

Opening conversation history could crash the app when a saved conversation title came from a user message containing CJK characters.

The panic was:

- `byte index ... is not a char boundary`

## Root cause

Conversation summary titles were truncated with raw byte slicing:

- `&m.content[..57]`

That is only valid for ASCII-safe boundaries. CJK characters are multibyte in UTF-8, so slicing at an arbitrary byte offset can panic.

## Fix applied

- Replaced byte-based truncation with character-safe truncation
- Added a regression test using Chinese text

## What we learned

- User-visible title and preview truncation must never use raw byte indexes on UTF-8 strings
- Any summary UI that handles arbitrary natural language should truncate by character boundary, not storage representation
