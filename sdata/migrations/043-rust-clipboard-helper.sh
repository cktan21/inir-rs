#!/usr/bin/env bash

MIGRATION_ID="043-rust-clipboard-helper"
MIGRATION_TITLE="Use the compiled clipboard utility"
MIGRATION_DESCRIPTION="Replaces iNiR's Python clipboard-store watcher with the byte-compatible Rust helper. Other/custom clipboard commands and existing history stay unchanged."
MIGRATION_TARGET_FILE="~/.config/niri/config.d/50-startup.kdl"
MIGRATION_REQUIRED=true
MIGRATION_SESSION_IMPACT=true
MIGRATION_SESSION_REFERENCE="Niri clipboard text watcher"
MIGRATION_SESSION_REASON="The clipboard watcher is spawned by Niri at login."
MIGRATION_SESSION_EFFECT="The current watcher keeps using Python until the next login."
MIGRATION_SESSION_ACTION="Log out and back in when convenient; no process is killed by this migration."

_rust_clipboard_startup="${HOME}/.config/niri/config.d/50-startup.kdl"
migration_check() {
    [[ -f "$_rust_clipboard_startup" ]] || return 1
    grep -Eq '^[[:space:]]*spawn-at-startup .*wl-paste .*--type text(/plain)? .*--watch ~/.config/quickshell/inir/scripts/clipboard-store\.py([[:space:]&]|$)' "$_rust_clipboard_startup"
}
migration_preview() {
    printf '%s\n' '- --watch ~/.config/quickshell/inir/scripts/clipboard-store.py' \
        '+ --watch ~/.config/quickshell/inir/scripts/inir-helper clipboard store' \
        'Only the managed iNiR text watcher is changed. No clipboard process is restarted.'
}
migration_apply() {
    local helper="${XDG_CONFIG_HOME:-$HOME/.config}/quickshell/inir/scripts/inir-helper"
    [[ -x "$helper" && "$("$helper" --protocol-version 2>/dev/null)" == 1 ]] || return 1
    [[ -f "$_rust_clipboard_startup" ]] || return 1
    sed -i -E '/^[[:space:]]*spawn-at-startup .*wl-paste .*--type text(\/plain)? .*--watch ~\/.config\/quickshell\/inir\/scripts\/clipboard-store\.py([[:space:]\&]|$)/ s|~/.config/quickshell/inir/scripts/clipboard-store\.py|~/.config/quickshell/inir/scripts/inir-helper clipboard store|' "$_rust_clipboard_startup"
    ! migration_check
}
