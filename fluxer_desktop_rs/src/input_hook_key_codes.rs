// SPDX-License-Identifier: AGPL-3.0-or-later
// Port of fluxer_desktop/src/main/GlobalShortcutKeys.ts Windows identity maps.

#[cfg(windows)]
fn windows_code(scan: u32, extended: bool, vk: u32) -> Option<&'static str> {
    let physical = if extended { 0xe000 | scan } else { scan };
    let code = match physical {
        0x0001 => Some("Escape"),
        0x0002 => Some("Digit1"),
        0x0003 => Some("Digit2"),
        0x0004 => Some("Digit3"),
        0x0005 => Some("Digit4"),
        0x0006 => Some("Digit5"),
        0x0007 => Some("Digit6"),
        0x0008 => Some("Digit7"),
        0x0009 => Some("Digit8"),
        0x000a => Some("Digit9"),
        0x000b => Some("Digit0"),
        0x000c => Some("Minus"),
        0x000d => Some("Equal"),
        0x000e => Some("Backspace"),
        0x000f => Some("Tab"),
        0x0010 => Some("KeyQ"),
        0x0011 => Some("KeyW"),
        0x0012 => Some("KeyE"),
        0x0013 => Some("KeyR"),
        0x0014 => Some("KeyT"),
        0x0015 => Some("KeyY"),
        0x0016 => Some("KeyU"),
        0x0017 => Some("KeyI"),
        0x0018 => Some("KeyO"),
        0x0019 => Some("KeyP"),
        0x001a => Some("BracketLeft"),
        0x001b => Some("BracketRight"),
        0x001c => Some("Enter"),
        0x001d => Some("ControlLeft"),
        0x001e => Some("KeyA"),
        0x001f => Some("KeyS"),
        0x0020 => Some("KeyD"),
        0x0021 => Some("KeyF"),
        0x0022 => Some("KeyG"),
        0x0023 => Some("KeyH"),
        0x0024 => Some("KeyJ"),
        0x0025 => Some("KeyK"),
        0x0026 => Some("KeyL"),
        0x0027 => Some("Semicolon"),
        0x0028 => Some("Quote"),
        0x0029 => Some("Backquote"),
        0x002a => Some("ShiftLeft"),
        0x002b => Some("Backslash"),
        0x002c => Some("KeyZ"),
        0x002d => Some("KeyX"),
        0x002e => Some("KeyC"),
        0x002f => Some("KeyV"),
        0x0030 => Some("KeyB"),
        0x0031 => Some("KeyN"),
        0x0032 => Some("KeyM"),
        0x0033 => Some("Comma"),
        0x0034 => Some("Period"),
        0x0035 => Some("Slash"),
        0x0036 => Some("ShiftRight"),
        0x0037 => Some("NumpadMultiply"),
        0x0038 => Some("AltLeft"),
        0x0039 => Some("Space"),
        0x003a => Some("CapsLock"),
        0x003b => Some("F1"),
        0x003c => Some("F2"),
        0x003d => Some("F3"),
        0x003e => Some("F4"),
        0x003f => Some("F5"),
        0x0040 => Some("F6"),
        0x0041 => Some("F7"),
        0x0042 => Some("F8"),
        0x0043 => Some("F9"),
        0x0044 => Some("F10"),
        0x0045 => Some("Pause"),
        0x0046 => Some("ScrollLock"),
        0x0047 => Some("Numpad7"),
        0x0048 => Some("Numpad8"),
        0x0049 => Some("Numpad9"),
        0x004a => Some("NumpadSubtract"),
        0x004b => Some("Numpad4"),
        0x004c => Some("Numpad5"),
        0x004d => Some("Numpad6"),
        0x004e => Some("NumpadAdd"),
        0x004f => Some("Numpad1"),
        0x0050 => Some("Numpad2"),
        0x0051 => Some("Numpad3"),
        0x0052 => Some("Numpad0"),
        0x0053 => Some("NumpadDecimal"),
        0x0054 => Some("PrintScreen"),
        0x0056 => Some("IntlBackslash"),
        0x0057 => Some("F11"),
        0x0058 => Some("F12"),
        0x0059 => Some("NumpadEqual"),
        0x0064 => Some("F13"),
        0x0065 => Some("F14"),
        0x0066 => Some("F15"),
        0x0067 => Some("F16"),
        0x0068 => Some("F17"),
        0x0069 => Some("F18"),
        0x006a => Some("F19"),
        0x006b => Some("F20"),
        0x006c => Some("F21"),
        0x006d => Some("F22"),
        0x006e => Some("F23"),
        0x0070 => Some("KanaMode"),
        0x0071 => Some("Lang2"),
        0x0072 => Some("Lang1"),
        0x0073 => Some("IntlRo"),
        0x0076 => Some("F24"),
        0x0077 => Some("Lang4"),
        0x0078 => Some("Lang3"),
        0x0079 => Some("Convert"),
        0x007b => Some("NonConvert"),
        0x007d => Some("IntlYen"),
        0x007e => Some("NumpadComma"),
        0xe008 => Some("Undo"),
        0xe00a => Some("Paste"),
        0xe010 => Some("MediaTrackPrevious"),
        0xe017 => Some("Cut"),
        0xe018 => Some("Copy"),
        0xe019 => Some("MediaTrackNext"),
        0xe01c => Some("NumpadEnter"),
        0xe01d => Some("ControlRight"),
        0xe020 => Some("AudioVolumeMute"),
        0xe021 => Some("LaunchApp2"),
        0xe022 => Some("MediaPlayPause"),
        0xe024 => Some("MediaStop"),
        0xe02c => Some("Eject"),
        0xe02e => Some("AudioVolumeDown"),
        0xe030 => Some("AudioVolumeUp"),
        0xe032 => Some("BrowserHome"),
        0xe035 => Some("NumpadDivide"),
        0xe037 => Some("PrintScreen"),
        0xe038 => Some("AltRight"),
        0xe03b => Some("Help"),
        0xe045 => Some("NumLock"),
        0xe046 => Some("Pause"),
        0xe047 => Some("Home"),
        0xe048 => Some("ArrowUp"),
        0xe049 => Some("PageUp"),
        0xe04b => Some("ArrowLeft"),
        0xe04d => Some("ArrowRight"),
        0xe04f => Some("End"),
        0xe050 => Some("ArrowDown"),
        0xe051 => Some("PageDown"),
        0xe052 => Some("Insert"),
        0xe053 => Some("Delete"),
        0xe05b => Some("MetaLeft"),
        0xe05c => Some("MetaRight"),
        0xe05d => Some("ContextMenu"),
        0xe05e => Some("Power"),
        0xe05f => Some("Sleep"),
        0xe063 => Some("WakeUp"),
        0xe065 => Some("BrowserSearch"),
        0xe066 => Some("BrowserFavorites"),
        0xe067 => Some("BrowserRefresh"),
        0xe068 => Some("BrowserStop"),
        0xe069 => Some("BrowserForward"),
        0xe06a => Some("BrowserBack"),
        0xe06b => Some("LaunchApp1"),
        0xe06c => Some("LaunchMail"),
        0xe06d => Some("MediaSelect"),
        _ => None,
    };
    code.or_else(|| match vk {
        0x08 => Some("Backspace"),
        0x09 => Some("Tab"),
        0x0c => Some("Numpad5"),
        0x0d => Some("Enter"),
        0x10 => Some("ShiftLeft"),
        0x11 => Some("ControlLeft"),
        0x12 => Some("AltLeft"),
        0x13 => Some("Pause"),
        0x14 => Some("CapsLock"),
        0x15 => Some("KanaMode"),
        0x1b => Some("Escape"),
        0x1c => Some("Convert"),
        0x1d => Some("NonConvert"),
        0x20 => Some("Space"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x23 => Some("End"),
        0x24 => Some("Home"),
        0x25 => Some("ArrowLeft"),
        0x26 => Some("ArrowUp"),
        0x27 => Some("ArrowRight"),
        0x28 => Some("ArrowDown"),
        0x2c => Some("PrintScreen"),
        0x2d => Some("Insert"),
        0x2e => Some("Delete"),
        0x5b => Some("MetaLeft"),
        0x5c => Some("MetaRight"),
        0x5d => Some("ContextMenu"),
        0x5f => Some("Sleep"),
        0x6a => Some("NumpadMultiply"),
        0x6b => Some("NumpadAdd"),
        0x6c => Some("NumpadComma"),
        0x6d => Some("NumpadSubtract"),
        0x6e => Some("NumpadDecimal"),
        0x6f => Some("NumpadDivide"),
        0x90 => Some("NumLock"),
        0x91 => Some("ScrollLock"),
        0x92 => Some("NumpadEqual"),
        0xa0 => Some("ShiftLeft"),
        0xa1 => Some("ShiftRight"),
        0xa2 => Some("ControlLeft"),
        0xa3 => Some("ControlRight"),
        0xa4 => Some("AltLeft"),
        0xa5 => Some("AltRight"),
        0xa6 => Some("BrowserBack"),
        0xa7 => Some("BrowserForward"),
        0xa8 => Some("BrowserRefresh"),
        0xa9 => Some("BrowserStop"),
        0xaa => Some("BrowserSearch"),
        0xab => Some("BrowserFavorites"),
        0xac => Some("BrowserHome"),
        0xad => Some("AudioVolumeMute"),
        0xae => Some("AudioVolumeDown"),
        0xaf => Some("AudioVolumeUp"),
        0xb0 => Some("MediaTrackNext"),
        0xb1 => Some("MediaTrackPrevious"),
        0xb2 => Some("MediaStop"),
        0xb3 => Some("MediaPlayPause"),
        0xb4 => Some("LaunchMail"),
        0xb5 => Some("MediaSelect"),
        0xb6 => Some("LaunchApp1"),
        0xb7 => Some("LaunchApp2"),
        0xba => Some("Semicolon"),
        0xbb => Some("Equal"),
        0xbc => Some("Comma"),
        0xbd => Some("Minus"),
        0xbe => Some("Period"),
        0xbf => Some("Slash"),
        0xc0 => Some("Backquote"),
        0xdb => Some("BracketLeft"),
        0xdc => Some("Backslash"),
        0xdd => Some("BracketRight"),
        0xde => Some("Quote"),
        0xe2 => Some("IntlBackslash"),
        _ => None,
    })
}

fn known_code(code: &str) -> bool {
    matches!(code,
        "AltLeft" |
        "AltRight" |
        "ArrowDown" |
        "ArrowLeft" |
        "ArrowRight" |
        "ArrowUp" |
        "AudioVolumeDown" |
        "AudioVolumeMute" |
        "AudioVolumeUp" |
        "Backquote" |
        "Backslash" |
        "Backspace" |
        "BracketLeft" |
        "BracketRight" |
        "BrowserBack" |
        "BrowserFavorites" |
        "BrowserForward" |
        "BrowserHome" |
        "BrowserRefresh" |
        "BrowserSearch" |
        "BrowserStop" |
        "CapsLock" |
        "Comma" |
        "ContextMenu" |
        "ControlLeft" |
        "ControlRight" |
        "Convert" |
        "Copy" |
        "Cut" |
        "Delete" |
        "Digit0" |
        "Digit1" |
        "Digit2" |
        "Digit3" |
        "Digit4" |
        "Digit5" |
        "Digit6" |
        "Digit7" |
        "Digit8" |
        "Digit9" |
        "Eject" |
        "End" |
        "Enter" |
        "Equal" |
        "Escape" |
        "F1" |
        "F10" |
        "F11" |
        "F12" |
        "F13" |
        "F14" |
        "F15" |
        "F16" |
        "F17" |
        "F18" |
        "F19" |
        "F2" |
        "F20" |
        "F21" |
        "F22" |
        "F23" |
        "F24" |
        "F3" |
        "F4" |
        "F5" |
        "F6" |
        "F7" |
        "F8" |
        "F9" |
        "Help" |
        "Home" |
        "Insert" |
        "IntlBackslash" |
        "IntlRo" |
        "IntlYen" |
        "KanaMode" |
        "KeyA" |
        "KeyB" |
        "KeyC" |
        "KeyD" |
        "KeyE" |
        "KeyF" |
        "KeyG" |
        "KeyH" |
        "KeyI" |
        "KeyJ" |
        "KeyK" |
        "KeyL" |
        "KeyM" |
        "KeyN" |
        "KeyO" |
        "KeyP" |
        "KeyQ" |
        "KeyR" |
        "KeyS" |
        "KeyT" |
        "KeyU" |
        "KeyV" |
        "KeyW" |
        "KeyX" |
        "KeyY" |
        "KeyZ" |
        "Lang1" |
        "Lang2" |
        "Lang3" |
        "Lang4" |
        "LaunchApp1" |
        "LaunchApp2" |
        "LaunchMail" |
        "MediaPlayPause" |
        "MediaSelect" |
        "MediaStop" |
        "MediaTrackNext" |
        "MediaTrackPrevious" |
        "MetaLeft" |
        "MetaRight" |
        "Minus" |
        "NonConvert" |
        "NumLock" |
        "Numpad0" |
        "Numpad1" |
        "Numpad2" |
        "Numpad3" |
        "Numpad4" |
        "Numpad5" |
        "Numpad6" |
        "Numpad7" |
        "Numpad8" |
        "Numpad9" |
        "NumpadAdd" |
        "NumpadComma" |
        "NumpadDecimal" |
        "NumpadDivide" |
        "NumpadEnter" |
        "NumpadEqual" |
        "NumpadMultiply" |
        "NumpadSubtract" |
        "PageDown" |
        "PageUp" |
        "Paste" |
        "Pause" |
        "Period" |
        "Power" |
        "PrintScreen" |
        "Quote" |
        "ScrollLock" |
        "Semicolon" |
        "ShiftLeft" |
        "ShiftRight" |
        "Slash" |
        "Sleep" |
        "Space" |
        "Tab" |
        "Undo" |
        "WakeUp"
    )
}

fn dom_code(key: &str) -> Option<String> {
    if key.len() == 1 {
        let c = key.as_bytes()[0].to_ascii_uppercase();
        if c.is_ascii_alphabetic() { return Some(format!("Key{}", c as char)); }
        if c.is_ascii_digit() { return Some(format!("Digit{}", c as char)); }
    }
    let named = match key {
        " " | "Spacebar" => "Space", "Esc" => "Escape", "Return" => "Enter",
        "Break" => "Pause", "Clear" => "NumLock", "Apps" => "ContextMenu",
        "Up" => "ArrowUp", "Down" => "ArrowDown", "Left" => "ArrowLeft", "Right" => "ArrowRight",
        "Del" => "Delete", "LaunchMediaPlayer" => "MediaSelect",
        "-" | "_" => "Minus", "=" | "+" => "Equal", "," | "<" => "Comma", "." | ">" => "Period",
        ";" | ":" => "Semicolon", "'" | "\"" => "Quote", "/" | "?" => "Slash",
        "\\" | "|" => "Backslash", "[" | "{" => "BracketLeft", "]" | "}" => "BracketRight", "`" | "~" => "Backquote",
        _ if known_code(key) => key,
        _ => return None,
    };
    Some(named.into())
}
