# Global controls and portable settings

Default global shortcuts:

- `Ctrl+Shift+M`: toggle microphone mute.
- `Ctrl+Shift+D`: toggle deafen. Deafening stops playback and microphone transmission; undeafening restores the existing microphone-mute choice.

Both use the same control handlers as the buttons and work in normal/performance mode. They act only during a connected voice session, not during connection setup, shutdown or the local microphone test. Holding a key does not repeatedly toggle it: macOS/X11 events are filtered by press/release state; Windows registration uses `MOD_NOREPEAT`.

## Change and save controls

Open **Settings**. Edit the server address, nickname, and shortcut text, then choose **Save settings**. A hostname, IP address, or address with a port can be used. Examples of shortcut text include `Ctrl+Shift+M`, `Ctrl+Alt+D`, `F8`, and `CmdOrCtrl+Shift+M`. The latter uses Command on macOS and Control on other platforms.

The two actions must use different shortcuts. Invalid syntax is rejected. New keys are registered before old keys are released; an ordinary registration conflict preserves the previous working keys and leaves the file unchanged. The Settings page shows registration/save errors and the last shortcut received, so the keys can be checked while disconnected.

Global shortcuts use the native event loop on macOS and Windows, and X11 on Linux. Native Wayland is not supported by the selected hotkey library and is reported as unavailable; settings can still be saved there. The app's event bridge is driven by key presses, with no periodic hotkey polling.

## Playback volume

The slider directly below **Playback** controls this app from 0–100%, starting at 100%. It works before connecting, during a session, and during the local microphone test. It applies output gain immediately through an atomic value read by the playback callback, with a short ramp to avoid abrupt changes. No allocation or lock is added to the audio callback.

0% silences playback while continuing to drain queued samples. It does not mute the microphone or change system volume. Use **Settings → Save settings** to retain the volume for the next launch.

## JSON file

The default file is `settings.json` beside the local identity file. **Copy settings path** gives its exact location, and `--settings-path` prints it. On this Mac it is:

```text
/Users/jangj/Library/Application Support/io.Venator.TeamSpeakNative/settings.json
```

The portable format is shown in [`settings.example.json`](../settings.example.json):

```json
{
  "server_address": "ts.zirkumflex.io",
  "nickname": "Native Voice",
  "playback_volume_percent": 100,
  "keybinds": {
    "toggle_mute": "Ctrl+Shift+M",
    "toggle_deafen": "Ctrl+Shift+D"
  }
}
```

Copy the JSON to the settings location on another computer, or set `NATIVE_VOICE_SETTINGS` to a custom JSON path. After external edits, disconnect and choose **Reload JSON**, or restart. The file does not contain server/channel passwords, device identifiers, or the private TeamSpeak identity.

Saving is explicit. Missing files are created with defaults; missing known fields receive defaults. Invalid JSON, unknown field names, invalid shortcuts and out-of-range volume produce an error. Loading an invalid file does not rewrite it. A successful Save writes a complete temporary file and renames it into place; failed writes do not leave partially written settings. Saving the displayed values after a load error explicitly replaces the invalid file.

## Validation

All 26 tests pass, including settings round trips/defaults, invalid-file preservation, invalid/duplicate bindings, repeated-key filtering, failed control delivery, deafen/mute state, volume scaling, silence, and gain ramping. Clippy with warnings denied and macOS/Windows release builds pass. Native Mac inspection confirmed the slider, editable Settings fields and a saved 39% test volume in an isolated JSON profile. Editing that JSON and reloading restored a different server address, nickname and 73% volume; editing a keybind in Settings updated both the registered-key hint and JSON file. The user confirmed both global actions were detected with physical key presses while another app was focused, and the UI showed the received deafen action. App-targeted synthetic key injection did not exercise macOS's global shortcut path.

The disconnected test profile uses separate keys and a temporary settings path so validation does not change the user's active voice session or saved defaults. Windows global-hotkey/audio runtime validation and native Wayland support remain outside the completed checks.
